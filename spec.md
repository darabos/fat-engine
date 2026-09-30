We're creating a soft body game engine in Bevy. It is made up of two parts: logic and display.

On the logic side, each object has an asset name, a position and a rotation. Nothing more! The logic for moving around the objects will be added later.

On the display side, we load the 3D mesh from the `assets` directory according to the asset name. We use the same mesh for two different purposes:

- A rigid frame. The logic side position and rotation controls the frame. The frame is invisible, non-colliding and not affected by gravity.
- A soft body mesh. This mesh is visible, colliding and affected by gravity. Each vertex is a point mass connected to its neigbors with springs. In addition, the vertices are connected to the matching vertex of the rigid frame with a spring. This keeps the soft body mesh keep its general shape, and allows the logic side to control the soft body mesh.

# Physics

Use XPBD-like constraints without trying to be physically accurate.

```
for each physics step:

    1. Move frame according to game logic

    2. Predict vertex positions
         p += v * dt

    3. Apply frame constraints
         pull vertices toward frame positions

    4. Resolve collisions

    5. Repeat constraint iterations

    6. Derive velocities
         v = (p_new - p_old) / dt

    7. Update render mesh
```

There are three sources of forces:
- Vertices are connected to neighbors with springs.
- Vertices are connected to the frame with springs.
- We try to roughly preserve the volume of bodies. Calculate the volume of the soft body mesh and compare to its original volume. Push all vertices in or out to try to restore the original volume.

The mesh volume can be calculated as (A \dot (B \cross C)) / 6 for each ABC triangle, summed with a positive/negative sign depending on whether the triangle faces away from or toward the origin.

# Collisions

Self-collisions are ignored. For collisions between soft bodies, we use a vertex-mesh collision for each vertex of the current body. Vertex-mesh collisions are done with https://parry.rs/.

Create a demo where a few assets are loaded from `assets/animals` and placed on a plane in this engine. Let the player move one of the bodies with the arrow keys.

