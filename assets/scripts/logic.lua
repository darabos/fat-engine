-- Example of a grid-based game. The Rust engine allows arbitrary (non-integer)
-- movement of entities, so non-grid-based games are also possible. Even in this
-- grid-based game, we use fractional coordinates to control the smooth
-- transition between grid positions.
player = {
    -- Logical coordinates.
    x = 1,
    y = 1,
    rot = 0,
    -- Render coordinates.
    rx = 1,
    ry = 1,
    rrot = 0,
}
SIZE = 10
-- Like in PICO-8, `_init` is called on startup.
function _init()
    for i = 1, SIZE-2 do
      if i%1==0 then
        -- `game` is a global game object added to the scope by Rust.
        -- `add_entity` places the specified asset at the given x/y coordinates.
        game.add_entity("tree", i, 0)
        game.add_entity("tree", i, SIZE-1)
        game.add_entity("tree", 0, i)
        game.add_entity("tree", SIZE-1, i)
      end
    end
    for i = 3, SIZE-2 do
        game.add_entity(i%2==0 and "pumpkin" or "beholder", i, i)
    end
    -- `add_entity` returns a handle to the object,
    -- which can be later used to move it or remove it.
    player.entity = game.add_entity("squish1", player.x, player.y)
    -- The area we want to make visible. Due to the aspect ratio of the window,
    -- we will end up seeing more. But this area is guaranteed to be visible.
    game.set_camera_view(0, 0, 10, 10)
end

-- `_update` is called on every frame.
function _update()
    -- `btnp` returns true on the frame when the button was pressed down.
    -- (Like in PICO-8.)
    if game.btnp("up") then
        if player.y == 1 then
            player.ry = player.ry - 0.5
        else
            player.y = player.y - 1
        end
        player.rot = 180
        -- `set_rotation` sets the rotation of the LogicBody in degrees.
    end
    if game.btnp("down") then
        if player.y == SIZE-2 then
            player.ry = player.ry + 0.5
        else
            player.y = player.y + 1
        end
        player.rot = 0
    end
    if game.btnp("left") then
        if player.x == 1 then
            player.rx = player.rx - 0.5
        else
            player.x = player.x - 1
        end
        player.rot = 270
    end
    if game.btnp("right") then
        if player.x == SIZE-2 then
            player.rx = player.rx + 0.5
        else
            player.x = player.x + 1
        end
        player.rot = 90
    end
    player.rx = 0.9 * player.rx + 0.1 * player.x
    player.ry = 0.9 * player.ry + 0.1 * player.y
    rot_delta = ((player.rot - player.rrot + 180) % 360) - 180
    player.rrot = player.rrot + 0.1 * rot_delta
    -- `move_to` moves the LogicBody to the given position instantly.
    player.entity.move_to(player.rx, player.ry)
    player.entity.set_rotation(player.rrot)
end
