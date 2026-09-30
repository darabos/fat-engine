We're creating a soft body game engine in Bevy. It is made up of two parts: logic and display.

On the logic side, each object has an asset name, a position and a rotation. Nothing more! The logic for moving around the objects will be added later.

On the display side, we load the 3D mesh from the `assets` directory according to the asset name and turn it into a [Rapier](https://rapier.rs/docs/user_guides/rust/soft_bodies/) soft body.

# Physics

All physics is done by Rapier 0.36 (`rapier3d`), held in a single `PhysicsWorld` Bevy resource and stepped in `FixedUpdate`.

- Each mesh becomes a `SoftBodyBuilder::trimesh` soft body: every vertex is a particle, mesh edges are structural constraints, and shape matching holds the overall shape. Rapier handles gravity and collisions with the ground and other bodies.
- The logic side controls a body by setting the shape-matching target pose of its whole-body cluster (cluster 0) to the logic position and rotation. The particles are pulled toward the rest shape at that pose.
- After each step, the particle positions are copied into the render mesh.

Create a demo where a few assets are loaded from `assets/animals` and placed on a plane in this engine. Let the player move one of the bodies with the arrow keys.

