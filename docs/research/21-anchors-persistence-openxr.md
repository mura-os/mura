# Anchors, persistence, and the OpenXR spatial-entities spine
**Status:** research / architecture input  
**Scope:** physically located applications that survive process exit and reboot  
**Baseline:** OpenXR 1.1.49 spatial-entities family, ratified 2025-06-10

## Evidence discipline
- **Verified** means the cited specification, code, or vendor documentation states the claim.
- **Observed implementation** means cited code implements it, without implying a standard guarantee.
- **Inference** means a conclusion from verified facts that the source does not itself promise.
- **Proposal** means a spatial-os design choice.
- **Unknown** marks an answer not established by reviewed public sources.
- Local AsciiDoc is treated as primary specification text and checked against generated Registry pages.
- Vendor documentation is evidence for that vendor's published contract, not every device/release.
- A numeric limit is reported only when its source distinguishes total capacity from batch size.

## 1. Purpose
The required experience is: place content at a physical location, exit, reboot, return to the room,
and recover the content at the same physical location.
The EXT family supplies the application-facing object and persistence model.
It does not specify the map, relocalizer, correction filter, database, or encryption that makes it true.

**Verified.** Khronos added `XR_EXT_spatial_entity`, `_anchor`, `_plane_tracking`,
`_marker_tracking`, `_persistence`, and `_persistence_operations` as ratified multi-vendor
extensions in SDK 1.1.49 on 2025-06-10
([release](https://github.com/KhronosGroup/OpenXR-SDK/releases/tag/release-1.1.49);
[Registry](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XR_EXT_spatial_entity.html)).

**Design conclusion.** spatial-os needs two layers:
1. a map/anchor service owning relocalization, correction, storage, encryption, and policy; and
2. Monado's standards-facing translation of handles, snapshots, futures, and errors.

The service is the physical-truth authority.
The OpenXR state tracker is not the map database.

## 2. The EXT family object model (primary-source)
Primary local sources:
- [`ext_spatial_entity.adoc`](../../references/openxr-docs/specification/sources/chapters/extensions/ext/ext_spatial_entity.adoc)
- [`ext_spatial_anchor.adoc`](../../references/openxr-docs/specification/sources/chapters/extensions/ext/ext_spatial_anchor.adoc)
- [`ext_spatial_plane_tracking.adoc`](../../references/openxr-docs/specification/sources/chapters/extensions/ext/ext_spatial_plane_tracking.adoc)
- [`ext_spatial_marker_tracking.adoc`](../../references/openxr-docs/specification/sources/chapters/extensions/ext/ext_spatial_marker_tracking.adoc)
- [`ext_spatial_persistence.adoc`](../../references/openxr-docs/specification/sources/chapters/extensions/ext/ext_spatial_persistence.adoc)
- [`ext_spatial_persistence_operations.adoc`](../../references/openxr-docs/specification/sources/chapters/extensions/ext/ext_spatial_persistence_operations.adoc)

### 2.1 Entities, components, capabilities, and contexts
**Verified.** `XR_EXT_spatial_entity` models physical, virtual, and app-defined things as entities.
An entity has at most one component of each component type.
Capabilities such as anchors and planes produce entities with guaranteed and optional components
([entity specification](https://developer.android.com/develop/xr/openxr/extensions/XR_EXT_spatial_entity)).

The layers are:
- `XrSpatialCapabilityEXT`: an ability such as anchor creation or plane tracking.
- `XrSpatialComponentTypeEXT`: typed data/behavior attached to an entity.
- `XrSpatialCapabilityFeatureEXT`: optional configuration dimensions.
- `XrSpatialContextEXT`: session-scoped resources configured with capabilities and components.
- `XrSpatialEntityIdEXT`: an ID valid in one context.
- `XrSpatialEntityEXT`: a handle expressing continued interest in an entity.
- `XrSpatialSnapshotEXT`: immutable coherent entity/component data.

**Verified.** Entity IDs are not reused for different entities in a context and are not reused
across contexts in the same session, even for the same physical entity
([local primary source](../../references/openxr-docs/specification/sources/chapters/extensions/ext/ext_spatial_entity.adoc)).
They are therefore neither durable nor global identifiers.

**Verified.** Context creation requires at least one capability and at least one enabled component
per capability. Unsupported configurations return specific errors; permission systems may return
`XR_ERROR_PERMISSION_INSUFFICIENT`
([context creation](https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrCreateSpatialContextAsyncEXT.html)).

**Runtime implementation must provide:**
- capability/component/feature enumeration;
- validation of every capability configuration and pNext feature structure;
- context-scoped ID allocation and handle lifetime;
- permission checks before sensitive pipelines start;
- reference-counted activation of requested perception pipelines; and
- teardown when no remaining context needs those pipelines.

**Inference.** Context configuration is a resource and permission boundary, not just negotiation.

### 2.2 Snapshots: discovery versus update
**Verified.** Discovery is asynchronous:
`xrCreateSpatialDiscoverySnapshotAsyncEXT` returns `XrFutureEXT`, and the paired completion call
returns the snapshot. Requests may complete out of order, and the runtime may throttle for power or
thermal policy
([discovery](https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrCreateSpatialDiscoverySnapshotAsyncEXT.html)).

**Verified.** A snapshot is immutable and coherent for its lifetime
([snapshot](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XrSpatialSnapshotEXT.html)).
Discovery component filters include entities having at least one requested component.
Query component conditions return only entities having all requested components
([query](https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrQuerySpatialComponentDataEXT.html)).

There are two paths:
- **Discovery:** async, potentially expensive, “which entities match now?”
- **Update:** synchronous snapshot, “give current components for these known entity handles.”

`XrEventDataSpatialDiscoveryRecommendedEXT` is a cadence/warm-up hint.
It is not an entity-change event; update snapshots retrieve changed component data
([primary source](../../references/openxr-docs/specification/sources/chapters/extensions/ext/ext_spatial_entity.adoc)).

**Runtime implementation must provide:**
- point-in-time snapshot materialization;
- immutable typed arrays and variable-size spatial buffers;
- stable index alignment across IDs, states, and chained component lists;
- memory accounting and explicit snapshot destruction; and
- spatial-buffer lifetime of at least the containing snapshot
  ([buffer lifetime](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XrSpatialBufferIdEXT.html)).

### 2.3 Tracking states are not storage states
**Verified.** Entity tracking is `TRACKING`, `PAUSED`, or terminal `STOPPED`.
`PAUSED` may resume; `STOPPED` never may.
Failure to relocalize after tracking restart is an example requiring `STOPPED`
([state](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XrSpatialEntityTrackingStateEXT.html)).

**Verified.** Component output is valid only for `TRACKING`; query leaves a non-tracking entity's
component slot unchanged.
The persistence component is the explicit exception and is returned regardless of tracking state
([query](https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrQuerySpatialComponentDataEXT.html);
[persistence source](../../references/openxr-docs/specification/sources/chapters/extensions/ext/ext_spatial_persistence.adoc)).

**Runtime implementation must separate:**
- durable UUID presence;
- map record loaded into memory;
- candidate map identified;
- anchor localized;
- anchor actively tracked; and
- tracking permanently abandoned.

### 2.4 The `XR_EXT_future` pattern
**Verified.** Async entry points validate/schedule work and return `XrFutureEXT`.
`xrPollFutureEXT` reports pending/ready.
One operation-specific completion call extracts `futureResult` and domain result.
Completing early yields `XR_ERROR_FUTURE_PENDING_EXT`; completion/cancellation invalidates the future
([future](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XR_EXT_future.html)).

Spatial entities uses futures for:
- spatial-context creation;
- discovery-snapshot creation;
- persistence-context creation;
- persist; and
- unpersist.

Persistence completion separates `XrResult` from `XrSpatialPersistenceContextResultEXT`.
That allows storage-domain failure without misusing loader/session errors.

**Runtime implementation must provide:**
- thread-safe completion and cancellation;
- correct parent lifetime;
- exactly-once completion consumption;
- result ownership across IPC;
- client-death cleanup; and
- a distinction between “request accepted” and “operation committed.”

### 2.5 Anchors
**Verified.** `XR_EXT_spatial_anchor` adds `XR_SPATIAL_CAPABILITY_ANCHOR_EXT`.
Every anchor has `XR_SPATIAL_COMPONENT_TYPE_ANCHOR_EXT`, whose data is `XrPosef`
([anchor](https://developer.android.com/develop/xr/openxr/extensions/XR_EXT_spatial_anchor)).

`xrCreateSpatialAnchorEXT` takes:
- an anchor-enabled context;
- `baseSpace`;
- `time`; and
- a pose applied in that space at that time.

It returns both context-local entity ID and entity handle.
The handle feeds update snapshots; the ID supports query correlation and persistence.

**Verified.** The runtime should adjust an anchor pose independently of other anchors/spaces to
maintain its real-world mapping.
Anchors are unaffected by system recentering.
Separately anchored objects may shift relative to each other, so rigidly related content should
share an anchor
([anchor behavior](https://developer.android.com/develop/xr/openxr/extensions/XR_EXT_spatial_anchor)).

**Runtime implementation must:**
- sample placement against `baseSpace` at requested `time`;
- bind the measurement to map evidence;
- track/relocalize it;
- express its pose in each snapshot's base space/time;
- return `PAUSED` until usable; and
- free live tracking when no entity/space handle needs it.

**Important boundary.** EXT anchor poses are snapshot components, not automatically `XrSpace`.
Android's separate `XR_ANDROID_spatial_anchor_space` creates that bridge and lets its `XrSpace`
outlive the context
([Android anchor space](https://developer.android.com/develop/xr/openxr/extensions/XR_ANDROID_spatial_anchor_space)).

### 2.6 Plane tracking
**Verified.** Plane tracking guarantees `BOUNDED_2D` and `PLANE_ALIGNMENT`.
Optional components are `MESH_2D`, `POLYGON_2D`, and `PLANE_SEMANTIC_LABEL`
([plane source](../../references/openxr-docs/specification/sources/chapters/extensions/ext/ext_spatial_plane_tracking.adoc)).

The runtime exposes:
- center pose and width/height;
- upward/downward horizontal, vertical, or arbitrary alignment
  ([alignment](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XrSpatialPlaneAlignmentEXT.html));
- optional indexed 2D mesh;
- optional polygon relative to an origin in its XY plane; and
- optional uncategorized/floor/wall/ceiling/table label
  ([labels](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XrSpatialPlaneSemanticLabelEXT.html)).

**Verified.** Polygon vertices are counter-clockwise, may be concave, and must not self-intersect
([polygon](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XrSpatialPolygon2DDataEXT.html)).

**Runtime implementation must:**
- associate repeated observations with stable entities where possible;
- transform geometry into requested base space/time;
- own immutable buffers per snapshot; and
- return optional components only when advertised and enabled.

### 2.7 Persistence scopes, UUIDs, and ownership
**Verified.** `XR_EXT_spatial_persistence` adds a session-scoped
`XrSpatialPersistenceContextEXT`, a connection to persistent entity storage.
Persisted entities expose `XrUuid` plus persistence state
([persistence](https://developer.android.com/develop/xr/openxr/extensions/XR_EXT_spatial_persistence)).

**Verified scopes:**
- `SYSTEM_MANAGED_EXT`: read-only system-managed entities, correlated across contexts/reboots.
- `LOCAL_ANCHORS_EXT`: modifiable anchors for the same device, user, and app.

These definitions are normative
([scope](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XrSpatialPersistenceScopeEXT.html)).

**Inference.** Ownership is expressed through scope and permission, not a specified filesystem path.
The runtime owns representation/durability.
The app owns returned UUID bookkeeping and associated content.
The EXT API defines no app-metadata payload.

A persistence context is linked into spatial-context creation to discover its store.
That linkage is not required merely to persist an entity
([persist](https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrPersistSpatialEntityAsyncEXT.html)).

Discovery may enumerate all matching persisted entities or filter by an OR-set of UUIDs.
Other filters are ANDed with that set.
A requested UUID can produce:
- `LOADED` plus valid context ID, with `TRACKING` or `PAUSED`;
- `NOT_FOUND` plus null ID and `STOPPED`; or
- no snapshot row when presence cannot yet be determined.

That three-way distinction is normative
([UUID filter](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XrSpatialDiscoveryPersistenceUuidFilterEXT.html)).

`XR_EXT_spatial_persistence_operations` adds async persist/unpersist:
- persist accepts context plus context-local entity ID and returns a durable UUID;
- repeating persist in the same scope is idempotent and returns the appropriate UUID;
- persistence may wait for a newly created anchor to reach `TRACKING`;
- unpersist addresses UUID rather than live handle; and
- unpersist does not destroy already-live runtime objects.

These follow the normative
[`persist`](https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrPersistSpatialEntityAsyncEXT.html)
and [`unpersist`](https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrUnpersistSpatialEntityAsyncEXT.html)
contracts.

**Runtime implementation must provide:**
- transactional UUID allocation;
- idempotency;
- per-scope authorization;
- durable commit before success;
- tombstone/not-found behavior;
- crash-safe unpersist; and
- durable UUID to newly allocated per-context ID correlation.

## 3. Shipping-runtime behavioral references
### 3.1 Android XR
**Verified publication, not independently tested behavior.** Android XR publishes the EXT entity,
anchor, persistence, and supported-extension pages
([index](https://developer.android.com/develop/xr/openxr/extensions)).
It also publishes cross-app/session `XR_ANDROID_device_anchor_persistence` with
`SCENE_UNDERSTANDING_COARSE`, and entity-bound anchors with a parent and stable surface-normal
distance
([bound anchor](https://developer.android.com/develop/xr/openxr/extensions/XR_ANDROID_spatial_entity_bound_anchor)).
**Quirk.** `XR_ANDROID_spatial_anchor_space` anchors cannot use Android device-anchor persistence;
they use EXT persistence operations
([rule](https://developer.android.com/develop/xr/openxr/extensions/XR_ANDROID_spatial_anchor_space)).
**Unknown.** Reviewed pages give no total count, quota, duration, or accuracy guarantee;
`XR_ERROR_LIMIT_REACHED` does not establish a numeric limit.
### 3.2 Meta's FB/META lineage
**Verified.** `XR_FB_spatial_entity` established components where `LOCATABLE` enables
`xrLocateSpace`, `STORABLE` enables save/erase, active entities are `XrSpace`, and UUIDs outlive
session handles
([Meta](https://developers.meta.com/horizon/documentation/native/android/openxr-spatial-anchors-api-ref/)).
It also established request-ID completion events, local/cloud locations
([enum](https://registry.khronos.org/OpenXR/specs/1.0/man/html/XrSpaceStorageLocationFB.html)),
query filters, erase separate from live-object destruction, and read-only system scene entities
([Scene API](https://latest.developers.meta.com/horizon/documentation/native/android/mobile-scene-api-ref/)).
**Inference.** EXT replaces vendor plumbing with configured contexts, snapshots, futures, and scopes
while retaining UUID and save/load/erase semantics.
**Shipping quirk.** Meta says creation has no maximum, but Unity and native UUID operations process
at most 50 UUIDs per call; this is a batch limit, not a verified persistence quota
([Unity](https://developers.meta.com/horizon/reference/unity/v69/class_o_v_r_spatial_anchor/);
[native](https://developers.meta.com/horizon/documentation/native/android/openxr-spatial-anchors-api-ref/)).
### 3.3 Microsoft lineage
**Verified.** `XR_MSFT_spatial_anchor_persistence` provides an app-scoped, app-named store
([store](https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrCreateSpatialAnchorStoreConnectionMSFT.html)).
Reusing a name replaces the record; enumeration uses two calls; unpersist leaves live handles valid
([persist](https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrPersistSpatialAnchorMSFT.html);
[unpersist](https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrUnpersistSpatialAnchorMSFT.html)).
**Inference.** EXT generalizes names to typed entities, runtime UUIDs, scopes, snapshots, and async I/O.
**Shipping quirk.** Microsoft gives no numeric local quota and warns persisted anchors retain sensor
data, reducing capacity for other anchors
([guidance](https://learn.microsoft.com/en-us/windows/mixed-reality/design/spatial-anchors)).

## 4. The map/local split: precedents and precise design
### 4.1 What precedents establish
**Verified.** ROS REP-105 defines `odom` as continuous but drifting and `map` as globally useful
but allowed to jump; localization publishes their correction
([REP-105](https://reps.openrobotics.org/rep-0105/)).
This is a frame-architecture precedent, not an XR product guarantee.
**Verified.** ARKit relocalization reconciles tracking before/after interruption.
On success, coordinates/anchors generally recover prior state; failure can leave them out of sync
([ARKit](https://developer.apple.com/documentation/arkit/artrackingstatereason/artrackingstatereasonrelocalizing)).
`ARWorldMap` packages mapping state plus anchors for a later session
([world map](https://developer.apple.com/documentation/arkit/arsession/getcurrentworldmap(completionhandler:))).
**Verified.** ARCore says every frame's world coordinates may differ and numerical locations of
**both camera and anchors** may change significantly as its model changes
([Pose](https://developers.google.com/ar/reference/java/com/google/ar/core/Pose)).
Rigid content should share a nearby anchor; beyond 8 m, rotational movement is a documented risk
([anchors](https://developers.google.com/ar/develop/anchors)).
**Correction.** “On loop closure, anchors move but camera pose does not” is not verified for
ARCore and conflicts with the first-party Pose documentation.
Maintainer commentary attributes jumps to loop closure but is not an API guarantee
([discussion](https://github.com/google-ar/arcore-android-sdk/issues/572)).
**Verified.** Cloud Anchors compare current visual features with a stored 3D feature map.
TTL is up to 365 days; uploaded visual data is discarded within 24 hours; API-key authorization
restricts TTL to 24 hours
([Cloud Anchors](https://developers.google.com/ar/develop/c/cloud-anchors/developer-guide)).
**Verified precedent.** Microsoft's World Locking Tools describes anchors moving on loop closure
and a spatial-frame/camera adjustment compensating
([FAQ](https://learn.microsoft.com/en-us/mixed-reality/world-locking-tools/documentation/introfaq)).
Patents describe transformed local maps and pose-graph correction, proving publication, not use
([patent](https://exa.ai/library/legal/patent/xb0hf8wll56pbyr5zfqx5t)).
### 4.2 Precise spatial-os transform design
**Proposal.** Let `T_A_B` transform coordinates from frame B into frame A.
- `local`: smooth gravity-aligned live VIO/render frame.
- `map`: optimized persistent frame; loop closure may change estimates.
- `keyframe`: stored observation frame.
- `anchor`: physical attachment frame.

Store and evaluate:
```text
T_map_anchor = T_map_keyframe · T_keyframe_anchor
T_local_anchor(t) = T_local_map(t) · T_map_anchor
```
High-rate VIO produces smooth `T_local_head(t)`.
Mapping/relocalization updates `T_map_keyframe` and estimates `T_local_map`.
Loop closure changes map-side quantities; rendering uses local VIO while anchors are re-expressed
through `T_local_map`.
**Inference.** This implements the EXT intent—anchor poses adjust to preserve physical
registration—without injecting pose-graph discontinuities into predicted head pose.
Correction policy can:
- apply immediately, maximizing physical correctness but visibly popping; or
- interpolate `T_local_map`, reducing the pop but temporarily allowing misalignment.
No filter guarantees perfect physical registration and zero visual motion after a discontinuity.
spatial-os should expose quality/correction state and use bounded, explicit policy.
### 4.3 Fit with OpenXR spaces
**Verified.** `xrLocateSpace` locates one space relative to another at requested time
([API](https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrLocateSpace.html)).
`LOCAL` is world-locked/gravity-aligned; `STAGE` adds a floor-centered bounded rectangle
([LOCAL](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XR_REFERENCE_SPACE_TYPE_LOCAL.html);
[STAGE](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XR_REFERENCE_SPACE_TYPE_STAGE.html)).

**Verified.** Redefining LOCAL/STAGE queues `XrEventDataReferenceSpaceChangePending`; old/new
definitions switch at `changeTime`
([event](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XrEventDataReferenceSpaceChangePending.html)).
**Important distinction.** That event covers reference-space origin/definition changes.
Anchor correction is represented by changed anchor component poses in update snapshots.
The specs do not require a reference-space-change event for each anchor correction.
**Proposal.** Keep routine map optimization behind stable LOCAL plus changing anchor components.
Use the reference-space event only for actual LOCAL/STAGE redefinition.
Any future anchor `XrSpace` must use the same map/local transform path as anchor snapshots.

## 5. Data model and store design
### 5.1 Evidence required for relocalization
**Verified precedent.** ORB-SLAM3 serializes an Atlas containing multiple maps/cameras.
Keyframes serialize calibration, keypoints, depth/stereo values, descriptors, bag-of-words data,
and graph relationships
([Atlas](https://github.com/UZ-SLAMLab/ORB_SLAM3/blob/master/include/Atlas.h);
[KeyFrame](https://github.com/UZ-SLAMLab/ORB_SLAM3/blob/ef9784101fbd28506b52f233315541ef8ba7af57/include/KeyFrame.h)).
Map points serialize position, normal, observations, descriptor, and reference IDs
([MapPoint](https://github.com/UZ-SLAMLab/ORB_SLAM3/blob/master/include/MapPoint.h)).

**Verified precedent.** RTAB-Map's SQLite schema stores nodes, links, words, features, descriptors,
and optional compressed image/depth blobs
([schema](https://github.com/introlab/rtabmap/blob/master/corelib/src/resources/DatabaseSchema.sql.in)).
It can trade retained descriptors against feature re-extraction from retained images
([maintainer](https://github.com/introlab/rtabmap/issues/1027)).

**Inference.** Offline relocalization minimally needs more than an anchor pose:
- calibrated keyframe camera model and timestamp;
- keyframe poses and covisibility/loop edges;
- local features and/or learned global descriptors;
- 2D observations associated with 3D landmarks;
- uncertainty, quality, and observation counts;
- vocabulary/model identity; and
- anchor-to-keyframe/entity transforms.

Raw images aid re-extraction and algorithm migration.
They are not logically required when descriptors/landmarks meet target recall.
Whether descriptor-only maps meet that target is a hardware/environment experiment, not a fact.

### 5.2 Proposed schema
**Proposal.** Use a versioned transactional database plus checksummed large blobs:
```text
Map { map_uuid, owner_user, label, timestamps, schema_version,
      algorithm_id, descriptor_model_id, calibration_id,
      state, quality, extent, active_generation }
Keyframe { keyframe_uuid, map_uuid, timestamp, T_map_keyframe,
           covariance, global_descriptor, feature_blob_ref, image_blob_ref? }
Landmark { landmark_uuid, map_uuid, position, normal?, descriptor,
           observation_count, quality }
Observation { keyframe_uuid, landmark_uuid, pixel, pyramid_level, uncertainty }
MapEdge { endpoints, relative_transform, covariance, edge_kind }
Anchor { persist_uuid, map_uuid, parent_keyframe_uuid?, parent_entity_uuid?,
         T_parent_anchor, covariance, quality, creator_app_id, timestamps, tombstone_generation }
AppBinding { app_id, persist_uuid, app_metadata_ciphertext?, created_at }
```

Do not conflate:
- durable `persist_uuid`;
- context-local `XrSpatialEntityIdEXT`;
- process handles;
- internal map/keyframe IDs; or
- app content IDs.

Only UUID crosses OpenXR sessions.
App metadata remains app-owned by default.
A runtime-side encrypted binding needs a separate future platform contract.

### 5.3 Multi-map / per-room
**Proposal.**
- An Atlas-like catalog contains independent room/zone maps.
- Each map has its own origin, compatibility metadata, descriptor version, and key.
- Topological edges may relate maps without merging metric frames.
- Relocalization searches global descriptors, then geometrically verifies candidates.
- Only selected/nearby maps enter active localization memory.
- Ambiguity keeps entities `PAUSED`; it must not guess a room.
- Labels are user convenience, never localization evidence.

This permits per-room deletion and handles repeated-looking spaces.
Map merge is copy-on-write until anchors migrate and verify.

### 5.4 Versioning, migration, durability
**Proposal.**
- Version schema, coordinates, calibration, extractor, vocabulary/model, and each blob.
- Make migrations copy-on-write and atomically switch active generation.
- Preserve one prior descriptor reader or require an explicit rescan.
- Use transactions/WAL and `fsync` before reporting persist success.
- Write an unpersist tombstone before garbage collection.
- Include WAL, blobs, thumbnails, diagnostics, and stale generations in deletion.
- Keep backups and wrapped keys separate from live data.

**Inference.** ORB-SLAM3 `.osa` is useful precedent, not a system format:
Boost object serialization is coupled to C++ types and code version
([implementation](https://github.com/UZ-SLAMLab/ORB_SLAM3/blob/master/src/System.cc)).

### 5.5 Size posture and ARWorldMap
**Verified.** Apple describes `ARWorldMap` as archivable mapping state plus anchors, but publishes
neither its internal representation nor fixed size limit
([ARWorldMap](https://developer.apple.com/documentation/arkit/arworldmap)).

**Anecdotal.** Developers report approximately 20–50 MB and trouble above 20 MB on some devices
([reports](https://stackoverflow.com/questions/60359368/swift-how-much-data-does-an-arkit-worldmap-take-up)).
These are sizing hints, not platform guarantees or suitable limits.

**Proposal.** Measure distributions per device/room, expose storage usage, and use user-visible
cleanup rather than undocumented eviction.

## 6. Privacy posture
Maps reveal a home's geometry and recognizable visual structure.
Descriptors are less directly viewable than images but still encode distinctive scenes.
Treat both as sensitive spatial data, not anonymous data.

**Verified.** Apple says ARKit world tracking processes sensor information on-device, requires
camera consent, and permits authorized apps to combine/store ARKit and camera data
([Apple](https://www.apple.com/legal/privacy/data/en/camera/)).
This is not a no-raw-images guarantee: `ARFrame.capturedImage` exposes a pixel buffer
([captured image](https://developer.apple.com/documentation/arkit/arframe/capturedimage)).

**Verified.** Meta requires app-specific permission before Scene API returns room data
([permission](https://developers.meta.com/horizon/documentation/native/native-spatial-data-perm/)).
Its separate camera API gives approved apps forward-camera frames and classifies them as Device
User Data
([camera](https://latest.developers.meta.com/horizon/documentation/unity/unity-pca-overview/)).

**Verified.** GDPR Article 25 requires protection by design/default and necessity limits on amount,
processing, retention, and accessibility
([Article 25](https://gdpr-info.eu/art-25-gdpr/)).
EDPB guidance names avoidance, access limitation, aggregation, pseudonymization, and deletion
([guidance](https://www.edpb.europa.eu/system/files/documents/files/file1/edpb_guidelines_201904_dataprotection_by_design_and_by_default_v2.0_en.pdf)).

**Proposal — spatial-os defaults:**
- On-device only.
- No map, descriptor, image, geometry, or UUID leaves the device without explicit sharing.
- Retain descriptors/landmarks rather than images when measured quality permits.
- Keep construction images only in bounded volatile storage and erase after extraction.
- If retained images are required, disclose per map and require opt-in.
- Encrypt each map with a per-map data-encryption key.
- Wrap keys with a device/user-bound key.
- TPM2 plus host-bound systemd credentials can require hardware and OS installation
  ([systemd](https://systemd.io/CREDENTIALS/)).
- Authorize entities by user, app, and scope.
- Provide list, storage usage, export, revoke, per-map delete, and delete-all controls.
- Delete derived meshes, descriptors, snapshots, logs, dumps, backups, and stale generations.
- Never log camera frames or descriptors; diagnostics are aggregate by default.

**Proposal — sharing boundary.** Sharing consumes a deliberately exported, separately encrypted
package with destination, categories, extent, expiry, and revocation UI.
“Enable anchors” must never imply “upload my room”.

## 7. Monado gap analysis
### 7.1 Existing reusable foundations
**Observed implementation.** This Monado tree supports `XR_EXT_future`.
`oxr_future_ext` wraps reference-counted `xrt_future`; completion invalidates it
([API](../../references/monado/src/xrt/state_trackers/oxr/oxr_api_future.c);
[implementation](../../references/monado/src/xrt/state_trackers/oxr/oxr_future.c)).
Monado documents xrt result registration, async callbacks, IPC future IDs, client wrappers, and
oxr completion pairing
([guide](../../references/monado/doc/async-functions-and-futures.md)).
**Observed implementation.** Older `XR_EXT_plane_detection` has xrt callbacks/capabilities,
orientation/semantics/extents/relations/polygons, base-space conversion, and IPC request paths
([xrt](../../references/monado/src/xrt/include/xrt/xrt_plane_detector.h);
[oxr](../../references/monado/src/xrt/state_trackers/oxr/oxr_api_session.c);
[IPC](../../references/monado/src/xrt/ipc/shared/proto/50-device.json)).
### 7.2 Missing implementation
**Observed implementation.** Extension support lists `XR_EXT_future` and `_plane_detection`, but
none of `_spatial_entity`, `_anchor`, `_spatial_plane_tracking`, `_persistence`, or
`_persistence_operations`
([support list](../../references/monado/src/xrt/state_trackers/oxr/extension_support/oxr_extension_support.py)).
**Required oxr:** negotiation/entry points; four new handle classes; context IDs; pNext parsing;
immutable snapshots/buffers; discovery events; and normative two-call/lifetime/error validation.
**Required xrt/service:** enumeration; pipeline activation; anchor/map-local poses; discovery;
snapshots; UUID persist/unpersist/authorization; and localization/tracking state.
**Required IPC:** IDs for every object/future, bounded variable arrays, async/client-death cleanup,
versioned messages, and no app-visible raw frames/database.
**Required map service:** transform authority, relocalizer/store, transactional encryption,
user/app policy/deletion, migration, and quality.
### 7.3 Reuse boundary for old plane plumbing
**Reusable:** capability flags, detector invocation/state, orientation/semantic mappings, extents,
relations, polygons, base-space transforms, and IPC variable arrays.
**Requires redesign:** request-as-identity, IDs expiring on the next request, `PENDING/DONE`
discovery, mutable result ownership, and one-HMD-device ownership.
The new family needs stable entities across snapshots, optional components, update handles,
persistence correlation, and shared service ownership.
Directly wrapping old results loses identity when requests restart or planes split/merge.
**Inference.** Reuse should sit below the entity service.
Adapt detector output into a canonical plane tracker that owns association/stable IDs, then snapshot
that tracker.
### 7.4 Upstream posture
**Proposal.** Upstream in dependency order:
1. generic spatial entity/snapshot xrt interfaces and oxr handles;
2. a plane adapter proving discovery/update/snapshot semantics;
3. anchor service interface and map/local poses;
4. persistence contexts and encrypted-store adapter; then
5. persistence operations and policy hooks.
Keep storage behind an implementation-neutral xrt interface.
Upstreamable code should contain standard objects, futures, snapshots, IPC, and backend contracts.
Distribution-specific UI, encryption policy, and store implementation may remain service-side.
## 8. Adopt
1. The ratified EXT family as the public entity/anchor/plane/local-persistence contract.
2. `XR_EXT_future` for long-running operations.
3. Context capabilities/components as activation and permission boundary.
4. Immutable snapshots as coherence boundary.
5. Runtime UUIDs distinct from context IDs, handles, map IDs, and app content IDs.
6. The map/local transform split in section 4.2.
7. Per-map, per-user, per-app encrypted storage.
8. Descriptors-not-images as the target, subject to measured recall.
9. Honest `TRACKING` / `PAUSED` / terminal `STOPPED`.
10. Old plane plumbing only below a stable entity tracker.
11. User-visible room maps, storage, deletion, and sharing consent.
## 9. Reject
1. Durable anchors represented as raw SLAM world coordinates.
2. Loop-closure jumps injected directly into predicted head pose.
3. Treating `XrSpatialEntityIdEXT` as persistent/global.
4. Runtime-owned app content metadata by default.
5. Retaining images merely because a reference database can.
6. Unencrypted maps or plaintext keys beside ciphertext.
7. Silent cloud backup, upload, or cross-app discovery.
8. Undocumented persisted-anchor eviction.
9. Claiming ARCore guarantees smooth camera while only anchors move.
10. Translating new plane entities directly to old per-request plane IDs.
11. Reporting persistence success before durable commit.
## 10. Open questions
1. Which SLAM backend exposes stable keyframe IDs, descriptors, covariance, and corrections?
2. What recall/latency target applies per room?
3. Can descriptor-only maps meet it across lighting, furniture, and device generations?
4. Is `T_local_map` immediate, interpolated, or selected by correction magnitude?
5. What correction triggers fade, system UI, or `PAUSED`?
6. How are repeated-looking rooms disambiguated without images or guessing?
7. What plane split/merge association policy is stable enough?
8. Does v1 expose only `LOCAL_ANCHORS_EXT`, or a system-managed room model too?
9. How are app/user identities stable across NixOS rebuilds?
10. What is key recovery after TPM/motherboard replacement?
11. What deletion SLA covers WAL, backup, diagnostics, and exports?
12. Does any app metadata belong in a platform API?
13. How will tests force cancellation, out-of-order completion, and client death during commit?
14. Can old plane backends support association, or must the canonical tracker match geometry?
15. Should anchor `XrSpace` wait for a ratified bridge?
16. Which Monado patches are upstreamable versus spatial-os service code?
The blocking spike is proving cold-start room relocalization, re-establishing `T_local_map`, and
returning physically correct anchors without exposing or indefinitely retaining raw home imagery.
