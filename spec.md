We're creating a soft body game engine in Bevy. It is made up of two parts: logic and display.

The logic side is driven by `assets/scripts/logic.lua`. Lua's optional `_init()` hook runs once at startup, and `_update()` runs once per rendered frame. The global `game` table provides `add_entity(asset, x, y)`, `set_camera_view(x, y, width, height)`, and `btnp(button)` (`up`, `down`, `left`, or `right`). Entity asset names are paths relative to `assets`; `.glb` is appended when omitted. Entity handles provide `move_to(x, y)`, `set_rotation(degrees)`, and `remove()`. Lua's x/y plane maps to world x/z, with rotation around the world y axis.

On the logic side, each object has an asset name, a position and a rotation. Nothing more!

On the display side, we load the 3D mesh from the `assets` directory according to the asset name and turn it into a [Rapier](https://rapier.rs/docs/user_guides/rust/soft_bodies/) soft body. All primitives of all meshes in the file are loaded, so an asset can be authored from several pieces with different materials.

# Physics

All physics is done by Rapier 0.36 (`rapier3d`), held in a single `PhysicsWorld` Bevy resource and stepped in `FixedUpdate`.

- The primitives of an asset are merged into one mesh, which is filled with a coarse cage of tetrahedral cells by `SoftBodyBuilder::volumetric_skinned`. Only the cage is simulated; the detailed mesh rides it as a *skin*. A mesh that isn't closed falls back to a shell of cells (`MeshEnclosure::Crust`).
- The body collides through the boundary of its cage, so contacts are as coarse as the cells. The cage is shrink-wrapped onto the mesh so it doesn't stand off it, and the contact skin is kept thin. Rapier handles gravity and all collisions.
- Overlapping bodies use Rapier's skin-aware intersection-volume contacts. The volume contact owns its patch; redundant point contacts within the patch stand down instead of fighting it. Contacts outside the patch still work normally.
- The logic side controls a body by setting the shape-matching target pose of its whole-body cluster (cluster 0) to the logic position and rotation. Every particle is pulled toward the rest shape at that pose, stiffly enough that gravity and collisions deform the body without moving it: the logic side owns the location and orientation, physics owns the wobble.
- The logic anchor is critically damped so held overlaps do not continually excite it. Cell springs retain their lighter damping for deformation and wobble.
- After each step, the skin vertices are read back and each primitive's render mesh is refreshed from its own slice of them.

Run `cargo test --lib physics::tests::overlapping_cages_fit_the_physics_frame_budget -- --ignored --nocapture` separately to benchmark moderate, deep, and coincident overlaps against a 60 Hz physics-step budget. This wall-clock check is excluded from normal tests.

Create a demo where a few assets are loaded from `assets/animals` and placed on a plane in this engine. Let the player move one of the bodies with the arrow keys.
