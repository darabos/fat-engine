We're creating a soft body game engine in Bevy. It is made up of two parts: logic and display.

On the logic side, each object has an asset name, a position and a rotation. Nothing more! The logic for moving around the objects will be added later.

On the display side, we load the 3D mesh from the `assets` directory according to the asset name and turn it into a [Rapier](https://rapier.rs/docs/user_guides/rust/soft_bodies/) soft body. All primitives of all meshes in the file are loaded, so an asset can be authored from several pieces with different materials.

# Physics

All physics is done by Rapier 0.36 (`rapier3d`), held in a single `PhysicsWorld` Bevy resource and stepped in `FixedUpdate`.

- The primitives of an asset are merged into one mesh, which is filled with a coarse cage of tetrahedral cells by `SoftBodyBuilder::volumetric_skinned`. Only the cage is simulated; the detailed mesh rides it as a *skin*. A mesh that isn't closed falls back to a shell of cells (`MeshEnclosure::Crust`).
- The body collides through the boundary of its cage, so contacts are as coarse as the cells. Rapier handles gravity and all collisions.
- The logic side controls a body by setting the shape-matching target pose of its whole-body cluster (cluster 0) to the logic position and rotation. The particles are pulled toward the rest shape at that pose.
- After each step, the skin vertices are read back and each primitive's render mesh is refreshed from its own slice of them.

Create a demo where a few assets are loaded from `assets/animals` and placed on a plane in this engine. Let the player move one of the bodies with the arrow keys.

