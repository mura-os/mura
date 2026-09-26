//! The renderer (specs/zxr-core.md §3 `render`, §6): ash on the runtime-created device. Planes
//! are textured quads drawn into the runtime's swapchain images with one depth buffer per view
//! (the 2D tier writes depth so the 3D tier slots in at M2). Client buffers become textures:
//! shm by one upload per commit, dmabuf by import with the buffer's modifier (zero CPU copies —
//! counted). Frame N+1 waits frame N's fence before the buffers frame N sampled are released,
//! so a client gets its buffer back when the GPU is done with it and not before (§6.5).

use crate::xr::math::Mat4;
use crate::xr::VkCore;
use ash::vk;
use std::os::fd::{AsRawFd, OwnedFd};

const PLANE_VERT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/plane.vert.spv"));
const PLANE_FRAG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/plane.frag.spv"));
const MAX_TEXTURES: u32 = 512;

#[repr(C)]
#[derive(Clone, Copy)]
struct PushConstants {
    mvp: Mat4,
    half_size: [f32; 2],
    uv_flip: [f32; 2],
}

pub struct Texture {
    pub image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    desc: vk::DescriptorSet,
    pub width: u32,
    pub height: u32,
    pub dmabuf: bool,
    /// shm only: staging buffer reused across uploads
    staging: Option<(vk::Buffer, vk::DeviceMemory, usize)>,
    layout_ready: bool,
}

pub struct PlaneDraw<'a> {
    pub model: Mat4,
    pub half_size: [f32; 2],
    pub texture: &'a Texture,
    pub flip_v: bool,
}

struct ViewTarget {
    extent: vk::Extent2D,
    depth_image: vk::Image,
    depth_memory: vk::DeviceMemory,
    depth_view: vk::ImageView,
    color_views: Vec<vk::ImageView>,
    framebuffers: Vec<vk::Framebuffer>,
}

pub struct FrameGpu {
    pub cmd: vk::CommandBuffer,
    pub fence: vk::Fence,
    pub in_use: bool,
}

pub struct Renderer {
    pub device: ash::Device,
    pub queue: vk::Queue,
    queue_family: u32,
    instance: ash::Instance,
    physical: vk::PhysicalDevice,
    mem_props: vk::PhysicalDeviceMemoryProperties,
    render_pass: vk::RenderPass,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    desc_layout: vk::DescriptorSetLayout,
    desc_pool: vk::DescriptorPool,
    sampler: vk::Sampler,
    cmd_pool: vk::CommandPool,
    targets: Vec<ViewTarget>,
    pub frames: Vec<FrameGpu>,
    query_pool: vk::QueryPool,
    timestamp_period_ns: f64,
    pub gpu_ns_last: u64,
    ext_mem_fd: ash::khr::external_memory_fd::Device,
    ext_fence_fd: ash::khr::external_fence_fd::Device,
}

fn find_memory_type(props: &vk::PhysicalDeviceMemoryProperties, type_bits: u32, flags: vk::MemoryPropertyFlags) -> Option<u32> {
    (0..props.memory_type_count).find(|&i| (type_bits & (1 << i)) != 0 && props.memory_types[i as usize].property_flags.contains(flags))
}

impl Renderer {
    pub fn new(core: &VkCore, color_format: vk::Format, extents: &[vk::Extent2D], swapchain_images: &[Vec<vk::Image>]) -> Result<Renderer, String> {
        let d = &core.device;
        let mem_props = unsafe { core.instance.get_physical_device_memory_properties(core.physical) };
        let props = unsafe { core.instance.get_physical_device_properties(core.physical) };
        unsafe {
            // render pass: colour (the runtime's image, kept in COLOR_ATTACHMENT_OPTIMAL as
            // XR_KHR_vulkan_enable2 requires) + a transient depth buffer
            let attachments = [
                vk::AttachmentDescription::default()
                    .format(color_format)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .initial_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .final_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL),
                vk::AttachmentDescription::default()
                    .format(vk::Format::D32_SFLOAT)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .initial_layout(vk::ImageLayout::UNDEFINED)
                    .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),
            ];
            let color_ref = [vk::AttachmentReference { attachment: 0, layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL }];
            let depth_ref = vk::AttachmentReference { attachment: 1, layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL };
            let subpass = [vk::SubpassDescription::default().pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS).color_attachments(&color_ref).depth_stencil_attachment(&depth_ref)];
            let deps = [vk::SubpassDependency::default()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS)
                .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS)
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)];
            let render_pass = d.create_render_pass(&vk::RenderPassCreateInfo::default().attachments(&attachments).subpasses(&subpass).dependencies(&deps), None).map_err(|e| e.to_string())?;

            // descriptors: one combined image sampler per texture
            let bindings = [vk::DescriptorSetLayoutBinding::default().binding(0).descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::FRAGMENT)];
            let desc_layout = d.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings), None).map_err(|e| e.to_string())?;
            let pool_sizes = [vk::DescriptorPoolSize { ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER, descriptor_count: MAX_TEXTURES }];
            let desc_pool = d.create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET).max_sets(MAX_TEXTURES).pool_sizes(&pool_sizes), None).map_err(|e| e.to_string())?;
            let sampler = d.create_sampler(&vk::SamplerCreateInfo::default().mag_filter(vk::Filter::LINEAR).min_filter(vk::Filter::LINEAR).address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE).address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE).address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE), None).map_err(|e| e.to_string())?;

            // pipeline
            let pc = [vk::PushConstantRange::default().stage_flags(vk::ShaderStageFlags::VERTEX).offset(0).size(std::mem::size_of::<PushConstants>() as u32)];
            let layouts = [desc_layout];
            let pipeline_layout = d.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts).push_constant_ranges(&pc), None).map_err(|e| e.to_string())?;
            let vert = d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&spirv_words(PLANE_VERT)), None).map_err(|e| e.to_string())?;
            let frag = d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&spirv_words(PLANE_FRAG)), None).map_err(|e| e.to_string())?;
            let stages = [
                vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::VERTEX).module(vert).name(c"main"),
                vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::FRAGMENT).module(frag).name(c"main"),
            ];
            let vi = vk::PipelineVertexInputStateCreateInfo::default();
            let ia = vk::PipelineInputAssemblyStateCreateInfo::default().topology(vk::PrimitiveTopology::TRIANGLE_LIST);
            let vp = vk::PipelineViewportStateCreateInfo::default().viewport_count(1).scissor_count(1);
            let rs = vk::PipelineRasterizationStateCreateInfo::default().polygon_mode(vk::PolygonMode::FILL).cull_mode(vk::CullModeFlags::NONE).line_width(1.0);
            let ms = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(vk::SampleCountFlags::TYPE_1);
            let ds = vk::PipelineDepthStencilStateCreateInfo::default().depth_test_enable(true).depth_write_enable(true).depth_compare_op(vk::CompareOp::LESS_OR_EQUAL);
            let blend_att = [vk::PipelineColorBlendAttachmentState::default()
                .blend_enable(true)
                .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .color_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(vk::BlendFactor::ONE)
                .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .alpha_blend_op(vk::BlendOp::ADD)
                .color_write_mask(vk::ColorComponentFlags::RGBA)];
            let cb = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_att);
            let dyn_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
            let dyn_state = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dyn_states);
            let pinfo = [vk::GraphicsPipelineCreateInfo::default()
                .stages(&stages)
                .vertex_input_state(&vi)
                .input_assembly_state(&ia)
                .viewport_state(&vp)
                .rasterization_state(&rs)
                .multisample_state(&ms)
                .depth_stencil_state(&ds)
                .color_blend_state(&cb)
                .dynamic_state(&dyn_state)
                .layout(pipeline_layout)
                .render_pass(render_pass)
                .subpass(0)];
            let pipeline = d.create_graphics_pipelines(vk::PipelineCache::null(), &pinfo, None).map_err(|(_, e)| e.to_string())?[0];
            d.destroy_shader_module(vert, None);
            d.destroy_shader_module(frag, None);

            let cmd_pool = d.create_command_pool(&vk::CommandPoolCreateInfo::default().queue_family_index(core.queue_family).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER), None).map_err(|e| e.to_string())?;
            let query_pool = d.create_query_pool(&vk::QueryPoolCreateInfo::default().query_type(vk::QueryType::TIMESTAMP).query_count(2), None).map_err(|e| e.to_string())?;

            let mut r = Renderer {
                device: d.clone(),
                queue: core.queue,
                queue_family: core.queue_family,
                instance: core.instance.clone(),
                physical: core.physical,
                mem_props,
                render_pass,
                pipeline_layout,
                pipeline,
                desc_layout,
                desc_pool,
                sampler,
                cmd_pool,
                targets: Vec::new(),
                frames: Vec::new(),
                query_pool,
                timestamp_period_ns: props.limits.timestamp_period as f64,
                gpu_ns_last: 0,
                ext_mem_fd: ash::khr::external_memory_fd::Device::new(&core.instance, d),
                ext_fence_fd: ash::khr::external_fence_fd::Device::new(&core.instance, d),
            };
            for (vi, ext) in extents.iter().enumerate() {
                r.targets.push(r.make_target(*ext, color_format, &swapchain_images[vi])?);
            }
            // two frames' worth of command buffers + fences (one in flight, one recording)
            let cmds = d.allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(cmd_pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(2)).map_err(|e| e.to_string())?;
            for cmd in cmds {
                let fence = d.create_fence(&vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED), None).map_err(|e| e.to_string())?;
                r.frames.push(FrameGpu { cmd, fence, in_use: false });
            }
            Ok(r)
        }
    }

    unsafe fn make_target(&self, extent: vk::Extent2D, color_format: vk::Format, images: &[vk::Image]) -> Result<ViewTarget, String> {
        let d = &self.device;
        let (depth_image, depth_memory) = self.create_image(extent.width, extent.height, vk::Format::D32_SFLOAT, vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT, vk::ImageTiling::OPTIMAL, None)?;
        let depth_view = d.create_image_view(&vk::ImageViewCreateInfo::default().image(depth_image).view_type(vk::ImageViewType::TYPE_2D).format(vk::Format::D32_SFLOAT).subresource_range(vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::DEPTH, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 }), None).map_err(|e| e.to_string())?;
        let mut color_views = Vec::new();
        let mut framebuffers = Vec::new();
        for img in images {
            let v = d.create_image_view(&vk::ImageViewCreateInfo::default().image(*img).view_type(vk::ImageViewType::TYPE_2D).format(color_format).subresource_range(vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 }), None).map_err(|e| e.to_string())?;
            let atts = [v, depth_view];
            let fb = d.create_framebuffer(&vk::FramebufferCreateInfo::default().render_pass(self.render_pass).attachments(&atts).width(extent.width).height(extent.height).layers(1), None).map_err(|e| e.to_string())?;
            color_views.push(v);
            framebuffers.push(fb);
        }
        Ok(ViewTarget { extent, depth_image, depth_memory, depth_view, color_views, framebuffers })
    }

    unsafe fn create_image(&self, w: u32, h: u32, format: vk::Format, usage: vk::ImageUsageFlags, tiling: vk::ImageTiling, mem_flags: Option<vk::MemoryPropertyFlags>) -> Result<(vk::Image, vk::DeviceMemory), String> {
        let d = &self.device;
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D { width: w, height: h, depth: 1 })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(tiling)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let image = d.create_image(&info, None).map_err(|e| e.to_string())?;
        let req = d.get_image_memory_requirements(image);
        let ty = find_memory_type(&self.mem_props, req.memory_type_bits, mem_flags.unwrap_or(vk::MemoryPropertyFlags::DEVICE_LOCAL)).ok_or("no device-local memory type")?;
        let memory = d.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(ty), None).map_err(|e| e.to_string())?;
        d.bind_image_memory(image, memory, 0).map_err(|e| e.to_string())?;
        Ok((image, memory))
    }

    fn make_texture_view(&self, image: vk::Image, format: vk::Format) -> Result<(vk::ImageView, vk::DescriptorSet), String> {
        unsafe {
            let d = &self.device;
            let view = d.create_image_view(&vk::ImageViewCreateInfo::default().image(image).view_type(vk::ImageViewType::TYPE_2D).format(format).subresource_range(vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 }), None).map_err(|e| e.to_string())?;
            let layouts = [self.desc_layout];
            let desc = d.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(self.desc_pool).set_layouts(&layouts)).map_err(|e| e.to_string())?[0];
            let img_info = [vk::DescriptorImageInfo { sampler: self.sampler, image_view: view, image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL }];
            let write = [vk::WriteDescriptorSet::default().dst_set(desc).dst_binding(0).descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).image_info(&img_info)];
            d.update_descriptor_sets(&write, &[]);
            Ok((view, desc))
        }
    }

    /// A texture for an shm buffer of `w`×`h`; the pixels arrive through `upload_shm`.
    pub fn create_shm_texture(&mut self, w: u32, h: u32) -> Result<Texture, String> {
        unsafe {
            let (image, memory) = self.create_image(w, h, vk::Format::B8G8R8A8_SRGB, vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST, vk::ImageTiling::OPTIMAL, None)?;
            let (view, desc) = self.make_texture_view(image, vk::Format::B8G8R8A8_SRGB)?;
            Ok(Texture { image, memory, view, desc, width: w, height: h, dmabuf: false, staging: None, layout_ready: false })
        }
    }

    /// One upload per commit: the shm pixels (ARGB8888/XRGB8888, `stride` bytes per row) into
    /// the texture through a host-visible staging buffer and a copy on the queue. This is the one
    /// CPU copy the design allows (shm has no other path); it is counted separately from dmabuf.
    pub fn upload_shm(&mut self, tex: &mut Texture, data: &[u8], stride: u32) -> Result<(), String> {
        unsafe {
            let d = &self.device;
            let size = (stride as usize) * (tex.height as usize);
            if tex.staging.map(|(_, _, s)| s < size).unwrap_or(true) {
                if let Some((b, m, _)) = tex.staging.take() {
                    d.destroy_buffer(b, None);
                    d.free_memory(m, None);
                }
                let buf = d.create_buffer(&vk::BufferCreateInfo::default().size(size as u64).usage(vk::BufferUsageFlags::TRANSFER_SRC).sharing_mode(vk::SharingMode::EXCLUSIVE), None).map_err(|e| e.to_string())?;
                let req = d.get_buffer_memory_requirements(buf);
                let ty = find_memory_type(&self.mem_props, req.memory_type_bits, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT).ok_or("no host-visible memory")?;
                let mem = d.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(ty), None).map_err(|e| e.to_string())?;
                d.bind_buffer_memory(buf, mem, 0).map_err(|e| e.to_string())?;
                tex.staging = Some((buf, mem, size));
            }
            let (buf, mem, _) = tex.staging.unwrap();
            let ptr = d.map_memory(mem, 0, size as u64, vk::MemoryMapFlags::empty()).map_err(|e| e.to_string())? as *mut u8;
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, size.min(data.len()));
            d.unmap_memory(mem);

            // one-shot copy; waits for completion (R0 simplicity — measured in the journal)
            let cmd = d.allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(self.cmd_pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1)).map_err(|e| e.to_string())?[0];
            d.begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)).map_err(|e| e.to_string())?;
            let old_layout = if tex.layout_ready { vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL } else { vk::ImageLayout::UNDEFINED };
            let to_dst = vk::ImageMemoryBarrier::default()
                .old_layout(old_layout)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_access_mask(vk::AccessFlags::SHADER_READ)
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .image(tex.image)
                .subresource_range(vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 });
            d.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::FRAGMENT_SHADER, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[to_dst]);
            let region = vk::BufferImageCopy::default()
                .buffer_offset(0)
                .buffer_row_length(stride / 4)
                .buffer_image_height(tex.height)
                .image_subresource(vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: 0, base_array_layer: 0, layer_count: 1 })
                .image_extent(vk::Extent3D { width: tex.width, height: tex.height, depth: 1 });
            d.cmd_copy_buffer_to_image(cmd, buf, tex.image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[region]);
            let to_read = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .image(tex.image)
                .subresource_range(vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 });
            d.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::FRAGMENT_SHADER, vk::DependencyFlags::empty(), &[], &[], &[to_read]);
            d.end_command_buffer(cmd).map_err(|e| e.to_string())?;
            let cmds = [cmd];
            let submit = [vk::SubmitInfo::default().command_buffers(&cmds)];
            let fence = d.create_fence(&vk::FenceCreateInfo::default(), None).map_err(|e| e.to_string())?;
            d.queue_submit(self.queue, &submit, fence).map_err(|e| e.to_string())?;
            d.wait_for_fences(&[fence], true, u64::MAX).map_err(|e| e.to_string())?;
            d.destroy_fence(fence, None);
            d.free_command_buffers(self.cmd_pool, &cmds);
            tex.layout_ready = true;
            Ok(())
        }
    }

    /// A dmabuf imported as a texture with its modifier (§6.2): no CPU copy. One plane only at
    /// R0 (ARGB/XRGB8888); the fourcc chooses the view format.
    pub fn import_dmabuf(&mut self, fd: &OwnedFd, w: u32, h: u32, fourcc: u32, modifier: u64, offset: u32, stride: u32) -> Result<Texture, String> {
        let format = match fourcc {
            0x3432_5241 /* AR24 */ | 0x3432_5258 /* XR24 */ => vk::Format::B8G8R8A8_SRGB,
            0x3432_4241 /* AB24 */ | 0x3432_4258 /* XB24 */ => vk::Format::R8G8B8A8_SRGB,
            other => return Err(format!("unsupported fourcc {other:#x}")),
        };
        unsafe {
            let d = &self.device;
            let layouts = [vk::SubresourceLayout { offset: offset as u64, size: 0, row_pitch: stride as u64, array_pitch: 0, depth_pitch: 0 }];
            let mut modinfo = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default().drm_format_modifier(modifier).plane_layouts(&layouts);
            let mut extmem = vk::ExternalMemoryImageCreateInfo::default().handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
            let info = vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(format)
                .extent(vk::Extent3D { width: w, height: h, depth: 1 })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
                .usage(vk::ImageUsageFlags::SAMPLED)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .push_next(&mut modinfo)
                .push_next(&mut extmem);
            let image = d.create_image(&info, None).map_err(|e| format!("vkCreateImage(dmabuf): {e}"))?;
            // the fd is consumed by the import: dup it
            let dup = libc::dup(fd.as_raw_fd());
            if dup < 0 {
                return Err("dup dmabuf fd".into());
            }
            let mut fdprops = vk::MemoryFdPropertiesKHR::default();
            self.ext_mem_fd.get_memory_fd_properties(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT, dup, &mut fdprops).map_err(|e| format!("vkGetMemoryFdPropertiesKHR: {e}"))?;
            let req = d.get_image_memory_requirements(image);
            let ty = find_memory_type(&self.mem_props, req.memory_type_bits & fdprops.memory_type_bits, vk::MemoryPropertyFlags::empty()).ok_or("no memory type for dmabuf")?;
            let mut import = vk::ImportMemoryFdInfoKHR::default().handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT).fd(dup);
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
            let memory = d.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(ty).push_next(&mut import).push_next(&mut dedicated), None).map_err(|e| format!("vkAllocateMemory(dmabuf): {e}"))?;
            d.bind_image_memory(image, memory, 0).map_err(|e| e.to_string())?;
            let (view, desc) = self.make_texture_view(image, format)?;
            Ok(Texture { image, memory, view, desc, width: w, height: h, dmabuf: true, staging: None, layout_ready: false })
        }
    }

    /// The format/modifier pairs the device samples from, for the dmabuf feedback table (§6.1).
    pub fn sampled_modifiers(&self, format: vk::Format) -> Vec<u64> {
        unsafe {
            let mut list = vk::DrmFormatModifierPropertiesListEXT::default();
            let mut props2 = vk::FormatProperties2::default().push_next(&mut list);
            self.instance.get_physical_device_format_properties2(self.physical, format, &mut props2);
            let n = list.drm_format_modifier_count as usize;
            let mut buf = vec![vk::DrmFormatModifierPropertiesEXT::default(); n];
            let mut list = vk::DrmFormatModifierPropertiesListEXT::default().drm_format_modifier_properties(&mut buf);
            let mut props2 = vk::FormatProperties2::default().push_next(&mut list);
            self.instance.get_physical_device_format_properties2(self.physical, format, &mut props2);
            buf.iter().filter(|p| p.drm_format_modifier_tiling_features.contains(vk::FormatFeatureFlags::SAMPLED_IMAGE) && p.drm_format_modifier_plane_count == 1).map(|p| p.drm_format_modifier).collect()
        }
    }

    pub fn destroy_texture(&mut self, tex: Texture) {
        unsafe {
            let d = &self.device;
            let _ = d.free_descriptor_sets(self.desc_pool, &[tex.desc]);
            d.destroy_image_view(tex.view, None);
            d.destroy_image(tex.image, None);
            d.free_memory(tex.memory, None);
            if let Some((b, m, _)) = tex.staging {
                d.destroy_buffer(b, None);
                d.free_memory(m, None);
            }
        }
    }

    /// Wait for frame slot `slot`'s previous submission (the one-frame-in-flight bound).
    pub fn wait_slot(&mut self, slot: usize) -> Result<(), String> {
        unsafe {
            if self.frames[slot].in_use {
                self.device.wait_for_fences(&[self.frames[slot].fence], true, u64::MAX).map_err(|e| e.to_string())?;
                self.frames[slot].in_use = false;
            }
            Ok(())
        }
    }

    /// Record and submit the scene pass for every view into the acquired swapchain images.
    /// `planes` are drawn for each view with that view's view-projection; dmabuf textures whose
    /// layout is still UNDEFINED get a one-time barrier to SHADER_READ_ONLY (the queue-family
    /// foreign path: the client's driver wrote them).
    pub fn render(&mut self, slot: usize, image_indices: &[u32], view_proj: &[Mat4], planes: &mut [PlaneDraw<'_>], clear: [f32; 4], transitions: &[vk::Image]) -> Result<(), String> {
        unsafe {
            let d = &self.device;
            let f = &self.frames[slot];
            let cmd = f.cmd;
            d.reset_fences(&[f.fence]).map_err(|e| e.to_string())?;
            d.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty()).map_err(|e| e.to_string())?;
            d.begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)).map_err(|e| e.to_string())?;
            d.cmd_reset_query_pool(cmd, self.query_pool, 0, 2);
            d.cmd_write_timestamp(cmd, vk::PipelineStageFlags::TOP_OF_PIPE, self.query_pool, 0);
            // Foreign-queue acquire for every dmabuf drawn this frame (wlroots' vulkan renderer
            // shape: the client's driver owns the image between our frames; GENERAL is the layout
            // a DRM-modifier image has outside Vulkan).
            let range = vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 };
            if !transitions.is_empty() {
                let barriers: Vec<vk::ImageMemoryBarrier> = transitions
                    .iter()
                    .map(|img| {
                        vk::ImageMemoryBarrier::default()
                            .old_layout(vk::ImageLayout::GENERAL)
                            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                            .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
                            .dst_queue_family_index(self.queue_family)
                            .dst_access_mask(vk::AccessFlags::SHADER_READ)
                            .image(*img)
                            .subresource_range(range)
                    })
                    .collect();
                d.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::FRAGMENT_SHADER, vk::DependencyFlags::empty(), &[], &[], &barriers);
            }
            for (vi, target) in self.targets.iter().enumerate() {
                let fb = target.framebuffers[image_indices[vi] as usize];
                let clears = [vk::ClearValue { color: vk::ClearColorValue { float32: clear } }, vk::ClearValue { depth_stencil: vk::ClearDepthStencilValue { depth: 1.0, stencil: 0 } }];
                d.cmd_begin_render_pass(cmd, &vk::RenderPassBeginInfo::default().render_pass(self.render_pass).framebuffer(fb).render_area(vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: target.extent }).clear_values(&clears), vk::SubpassContents::INLINE);
                d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
                d.cmd_set_viewport(cmd, 0, &[vk::Viewport { x: 0.0, y: 0.0, width: target.extent.width as f32, height: target.extent.height as f32, min_depth: 0.0, max_depth: 1.0 }]);
                d.cmd_set_scissor(cmd, 0, &[vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: target.extent }]);
                for p in planes.iter() {
                    let mvp = crate::xr::math::mul(&view_proj[vi], &p.model);
                    let pc = PushConstants { mvp, half_size: p.half_size, uv_flip: [0.0, if p.flip_v { 1.0 } else { 0.0 }] };
                    let bytes = std::slice::from_raw_parts(&pc as *const _ as *const u8, std::mem::size_of::<PushConstants>());
                    d.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline_layout, 0, &[p.texture.desc], &[]);
                    d.cmd_push_constants(cmd, self.pipeline_layout, vk::ShaderStageFlags::VERTEX, 0, bytes);
                    d.cmd_draw(cmd, 6, 1, 0, 0);
                }
                d.cmd_end_render_pass(cmd);
            }
            if !transitions.is_empty() {
                let barriers: Vec<vk::ImageMemoryBarrier> = transitions
                    .iter()
                    .map(|img| {
                        vk::ImageMemoryBarrier::default()
                            .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                            .new_layout(vk::ImageLayout::GENERAL)
                            .src_queue_family_index(self.queue_family)
                            .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
                            .src_access_mask(vk::AccessFlags::SHADER_READ)
                            .image(*img)
                            .subresource_range(range)
                    })
                    .collect();
                d.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::FRAGMENT_SHADER, vk::PipelineStageFlags::BOTTOM_OF_PIPE, vk::DependencyFlags::empty(), &[], &[], &barriers);
            }
            d.cmd_write_timestamp(cmd, vk::PipelineStageFlags::BOTTOM_OF_PIPE, self.query_pool, 1);
            d.end_command_buffer(cmd).map_err(|e| e.to_string())?;
            let cmds = [cmd];
            let submit = [vk::SubmitInfo::default().command_buffers(&cmds)];
            d.queue_submit(self.queue, &submit, f.fence).map_err(|e| e.to_string())?;
            self.frames[slot].in_use = true;
            Ok(())
        }
    }

    /// GPU time of the last completed pass (call after `wait_slot`).
    pub fn read_gpu_time(&mut self) -> Option<u64> {
        unsafe {
            let mut ts = [0u64; 2];
            self.device.get_query_pool_results(self.query_pool, 0, &mut ts, vk::QueryResultFlags::TYPE_64).ok()?;
            if ts[1] >= ts[0] {
                self.gpu_ns_last = ((ts[1] - ts[0]) as f64 * self.timestamp_period_ns) as u64;
                Some(self.gpu_ns_last)
            } else {
                None
            }
        }
    }

    /// Export a sync file for a fence (explicit-sync release plumbing; §6.5 alternative path).
    #[allow(dead_code)]
    pub fn fence_sync_fd(&self, fence: vk::Fence) -> Result<i32, String> {
        unsafe { self.ext_fence_fd.get_fence_fd(&vk::FenceGetFdInfoKHR::default().fence(fence).handle_type(vk::ExternalFenceHandleTypeFlags::SYNC_FD)).map_err(|e| e.to_string()) }
    }
}

fn spirv_words(bytes: &[u8]) -> Vec<u32> {
    bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            let d = &self.device;
            let _ = d.device_wait_idle();
            for f in &self.frames {
                d.destroy_fence(f.fence, None);
            }
            for t in &self.targets {
                for fb in &t.framebuffers {
                    d.destroy_framebuffer(*fb, None);
                }
                for v in &t.color_views {
                    d.destroy_image_view(*v, None);
                }
                d.destroy_image_view(t.depth_view, None);
                d.destroy_image(t.depth_image, None);
                d.free_memory(t.depth_memory, None);
            }
            d.destroy_query_pool(self.query_pool, None);
            d.destroy_command_pool(self.cmd_pool, None);
            d.destroy_pipeline(self.pipeline, None);
            d.destroy_pipeline_layout(self.pipeline_layout, None);
            d.destroy_sampler(self.sampler, None);
            d.destroy_descriptor_pool(self.desc_pool, None);
            d.destroy_descriptor_set_layout(self.desc_layout, None);
            d.destroy_render_pass(self.render_pass, None);
        }
    }
}
