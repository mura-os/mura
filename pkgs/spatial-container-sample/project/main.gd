# SPDX-License-Identifier: MIT
# Mura spatial-container sample — the conformance client of specs/composition.md §7.3.
#
# Derived from GodotVR/godot_openxr_vendors samples/spatial-container-sample/main.gd
# (Copyright (c) 2022-present Godot XR contributors, MIT — LICENSE.godot-xr-vendors). Kept
# from it: the robot, the bounded<->immersive timer, scaling content to the reported bounds.
# Added for Mura: the `SCS <event> ...` log lines a harness greps (one line per OpenXR
# event the container pair defines), a key to drive the bounds mode, an `--instance N`
# tint so two processes are told apart in the two-container gate (composition.md §7.5),
# a wireframe of the container bounds, and the detection idiom of GodotVR/spatialize
# (`Engine.get_singleton(...)` + `is_enabled()`) so the same binary runs unchanged on a
# runtime without XR_EXT_spatial_container — the C0 baseline on Monado today.
# The world-scale rule (`K / min(bounds)`) is m4gr3d's Starter-Kit `spatialize` branches'.
extends StartXR

const BOUNDS_MODES_SWITCH_TIME := 10.0  # seconds; the vendors sample's cadence
const LOG_PREFIX := "SCS"
# Where the content sits when no bounded container positions it: the vendors sample keeps
# it at the origin because a hosting runtime places the container in front of the user and
# the container space's origin is its centre (ext_spatial_container.adoc:278-290). With the
# extension absent (C0) or in immersive mode the origin is the LOCAL_FLOOR origin — the
# viewer's feet — and the simulated HMD at ~1.6 m sees nothing but the clear colour. So
# without a bounded container the content is parked 1 m ahead at chest height.
const IMMERSIVE_CONTENT_POSITION := Vector3(0.0, 1.3, -1.0)

var spatial_container_ext = null  # OpenXRSpatialContainerExtension singleton, when present
var supported_bounds_modes: Array = []
var current_bounds_mode_index := 0
var cycle_on_timer := true
var instance_index := 0

@onready var content: Node3D = $Content
@onready var godot_robot: Node3D = $Content/GodotRobot
@onready var marker: MeshInstance3D = $Content/Marker
@onready var bounds_box: MeshInstance3D = $Content/BoundsBox

var initial_robot_scale: Vector3
var bounds_to_scale_ratio: Vector3
var initial_bounds: Vector3


func _log(event: String, fields: Array = []) -> void:
	# `SCS <event> k=v k=v` — stable, greppable, one event per line.
	var parts := PackedStringArray([LOG_PREFIX, event])
	for f in fields:
		parts.append(str(f))
	print(" ".join(parts))


func _ready() -> void:
	_parse_args()
	super()

	initial_robot_scale = godot_robot.scale
	initial_bounds = ProjectSettings.get_setting_with_override("xr/openxr/extensions/spatial_container/bounds")
	bounds_to_scale_ratio = initial_robot_scale / initial_bounds
	_tint_marker()
	_update_bounds_box(initial_bounds)

	# spatialize's detection idiom: the singleton exists only when the project setting is on
	# and the runtime advertises the extension; on Monado today it is absent or disabled and
	# the session is an ordinary immersive one.
	if Engine.has_singleton("OpenXRSpatialContainerExtension"):
		spatial_container_ext = Engine.get_singleton("OpenXRSpatialContainerExtension")
	if spatial_container_ext == null or not spatial_container_ext.is_enabled():
		_log("ext", ["absent", "runtime=%s" % _runtime_name()])
		spatial_container_ext = null
		content.position = IMMERSIVE_CONTENT_POSITION
		bounds_box.visible = false
		return

	_log("ext", ["present", "runtime=%s" % _runtime_name(), "suggested_bounds=%s" % initial_bounds])
	spatial_container_ext.spatial_container_visible_changed.connect(_on_visible_changed)
	spatial_container_ext.spatial_container_visible_request_denied.connect(_on_visible_request_denied)
	spatial_container_ext.spatial_container_interactable_changed.connect(_on_interactable_changed)
	spatial_container_ext.spatial_container_bounds_changed.connect(_on_bounds_changed)
	spatial_container_ext.spatial_container_bounds_mode_request_denied.connect(_on_bounds_mode_request_denied)
	spatial_container_ext.spatial_container_closed.connect(_on_closed)


func _parse_args() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--instance="):
			instance_index = int(arg.trim_prefix("--instance="))
		elif arg == "--no-cycle":
			cycle_on_timer = false


func _runtime_name() -> String:
	var iface := XRServer.find_interface("OpenXR")
	if iface == null:
		return "none"
	# OpenXRInterface exposes the runtime name through the system name at 4.8.
	return str(iface.get_system_info().get("XRRuntimeName", "unknown")).replace(" ", "_")


func _tint_marker() -> void:
	# A small cube above the robot whose colour is the instance index — instance 0 white,
	# 1 red, 2 green, 3 blue — so the two-container gate can tell processes apart.
	var mesh := BoxMesh.new()
	mesh.size = Vector3(0.06, 0.06, 0.06)
	var mat := StandardMaterial3D.new()
	mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	var colours := [Color.WHITE, Color.RED, Color.GREEN, Color.DODGER_BLUE]
	mat.albedo_color = colours[instance_index % colours.size()]
	mesh.material = mat
	marker.mesh = mesh


func _update_bounds_box(bounds: Vector3) -> void:
	# The twelve edges of the container's bounding box, in the container space (origin at
	# the bounds centre, +X right, +Y up, +Z front — ext_spatial_container.adoc:278-290).
	var im := ImmediateMesh.new()
	var h := bounds * 0.5
	var c := [
		Vector3(-h.x, -h.y, -h.z), Vector3(h.x, -h.y, -h.z), Vector3(h.x, -h.y, h.z), Vector3(-h.x, -h.y, h.z),
		Vector3(-h.x, h.y, -h.z), Vector3(h.x, h.y, -h.z), Vector3(h.x, h.y, h.z), Vector3(-h.x, h.y, h.z),
	]
	var edges := [[0, 1], [1, 2], [2, 3], [3, 0], [4, 5], [5, 6], [6, 7], [7, 4], [0, 4], [1, 5], [2, 6], [3, 7]]
	var mat := StandardMaterial3D.new()
	mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	mat.albedo_color = Color(1.0, 0.8, 0.2)
	im.surface_begin(Mesh.PRIMITIVE_LINES, mat)
	for e in edges:
		im.surface_add_vertex(c[e[0]])
		im.surface_add_vertex(c[e[1]])
	im.surface_end()
	bounds_box.mesh = im


func _process(_delta: float) -> void:
	marker.rotate_y(0.02)


func _unhandled_key_input(event: InputEvent) -> void:
	# `B` = request the next bounds mode; `Q` = quit. Lets a harness (or a person at the
	# dev-session's keyboard) drive the state machine without waiting for the timer.
	if event is InputEventKey and event.pressed and not event.echo:
		if event.keycode == KEY_B:
			_request_next_bounds_mode()
		elif event.keycode == KEY_Q:
			_log("quit", [])
			get_tree().quit()


# --- StartXR hooks ----------------------------------------------------------------------

func _on_openxr_session_begun() -> void:
	super()
	_log("session", ["begun"])
	if spatial_container_ext:
		_start_bounds_modes_timer()


func _on_openxr_visible_state() -> void:
	super()
	_log("session", ["visible"])


func _on_openxr_focused_state() -> void:
	super()
	_log("session", ["focused"])


func _on_openxr_stopping() -> void:
	_log("session", ["stopping"])
	super()


# --- container events --------------------------------------------------------------------

func _on_visible_changed(_rid: RID, is_visible: bool) -> void:
	_log("visible", [is_visible])


func _on_visible_request_denied(_rid: RID) -> void:
	_log("visible_request_denied", [])


func _on_interactable_changed(_rid: RID, is_interactable: bool) -> void:
	_log("interactable", [is_interactable])


func _on_bounds_changed(_rid: RID, infinite_bounds: bool, bounds_mode: OpenXRSpatialContainerState.BoundsMode, bounds: Vector3) -> void:
	_log("bounds", ["mode=%d" % bounds_mode, "infinite=%s" % infinite_bounds, "bounds=%s" % bounds])
	if infinite_bounds:
		# Immersive: the robot at its natural scale, in front of the viewer, the box hidden.
		godot_robot.scale = initial_robot_scale
		content.scale = Vector3.ONE
		content.position = IMMERSIVE_CONTENT_POSITION
		bounds_box.visible = false
	else:
		# Bounded: content at the container's centre; scale the robot to the reported
		# bounds (the vendors sample) and re-draw the box. The Starter-Kit rule
		# `K / min(bounds)` is the same idea applied to the XROrigin's world_scale — here
		# the content is scaled, the origin left alone.
		content.position = Vector3.ZERO
		godot_robot.scale = bounds * bounds_to_scale_ratio
		_update_bounds_box(bounds)
		bounds_box.visible = true


func _on_bounds_mode_request_denied(_rid: RID) -> void:
	_log("bounds_mode_request_denied", [])


func _on_closed(_rid: RID) -> void:
	_log("closed", [])
	get_tree().quit()


# --- bounds-mode cycling ----------------------------------------------------------------

func _start_bounds_modes_timer() -> void:
	supported_bounds_modes = spatial_container_ext.get_supported_bounds_modes()
	var state: OpenXRSpatialContainerState = spatial_container_ext.get_spatial_container_state()
	var current_mode := state.get_bounds_mode()
	current_bounds_mode_index = maxi(supported_bounds_modes.find(current_mode), 0)
	_log("caps", ["supported_bounds_modes=%s" % str(supported_bounds_modes), "mode=%d" % current_mode,
		"visible=%s" % state.is_visible(), "interactable=%s" % state.is_interactable(),
		"bounds=%s" % spatial_container_ext.get_spatial_container_bounds()])
	if cycle_on_timer and supported_bounds_modes.size() > 1:
		get_tree().create_timer(BOUNDS_MODES_SWITCH_TIME).timeout.connect(_on_bounds_modes_timer_timeout)


func _on_bounds_modes_timer_timeout() -> void:
	_request_next_bounds_mode()
	if cycle_on_timer:
		get_tree().create_timer(BOUNDS_MODES_SWITCH_TIME).timeout.connect(_on_bounds_modes_timer_timeout)


func _request_next_bounds_mode() -> void:
	if spatial_container_ext == null or supported_bounds_modes.size() < 2:
		_log("request_bounds_mode", ["skipped", "reason=%s" % ("no_ext" if spatial_container_ext == null else "one_mode")])
		return
	var next_index := (current_bounds_mode_index + 1) % supported_bounds_modes.size()
	var next_mode = supported_bounds_modes[next_index]
	var ok: bool = spatial_container_ext.request_spatial_container_bounds_mode(next_mode)
	_log("request_bounds_mode", ["mode=%d" % next_mode, "accepted_call=%s" % ok])
	current_bounds_mode_index = next_index
