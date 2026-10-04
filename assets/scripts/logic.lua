-- Example of a grid-based game. The Rust engine allows arbitrary (non-integer)
-- movement of entities, so non-grid-based games are also possible. Even in this
-- grid-based game, we use fractional coordinates to control the smooth
-- transition between grid positions.
player = {
    -- Logical coordinates.
    x = 1,
    y = 1,
    -- Render coordinates.
    rx = 1,
    ry = 1,
}
-- Like in PICO-8, `_init` is called on startup.
function _init()
    for i = 1, 10 do
      -- `game` is a global game object added to the scope by Rust.
      -- `add_entity` places the specified asset at the given x/y coordinates.
      game.add_entity("wall", i, 0)
      game.add_entity("wall", i, 9)
      game.add_entity("wall", 0, i)
      game.add_entity("wall", 9, i)
    end
    game.add_entity("goal", 8, 8)
    -- `add_entity` returns a handle to the object,
    -- which can be later used to move it or remove it.
    player.entity = game.add_entity("player", player.x, player.y)
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
        -- `set_rotation` sets the rotation of the LogicBody in degrees.
        player.entity.set_rotation(0)
    end
    if game.btnp("down") then
        if player.y == 8 then
            player.ry = player.ry + 0.5
        else
            player.y = player.y + 1
        end
        player.entity.set_rotation(180)
    end
    if game.btnp("left") then
        if player.x == 1 then
            player.rx = player.rx - 0.5
        else
            player.x = player.x - 1
        end
        player.entity.set_rotation(270)
    end
    if game.btnp("right") then
        if player.x == 8 then
            player.rx = player.rx + 0.5
        else
            player.x = player.x + 1
        end
        player.entity.set_rotation(90)
    end
    player.rx = 0.9 * player.rx + 0.1 * player.x
    player.ry = 0.9 * player.ry + 0.1 * player.y
    -- `move_to` moves the LogicBody to the given position instantly.
    player.entity.move_to(player.rx, player.ry)
end
