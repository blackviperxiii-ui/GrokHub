-- Loadout stats. Listens to a few game events and writes a summary to
-- script-output/loadout/state.json on each connected player's own computer.
-- It adds no entities, recipes, settings or GUI and changes nothing in the
-- game. The only state it keeps is a death count per player.

local FILE = "loadout/state.json"
local EVERY = 60 * 60 * 5 -- five minutes of game ticks
local ITEMS = {
  "iron-plate",
  "copper-plate",
  "steel-plate",
  "electronic-circuit",
  "advanced-circuit",
  "processing-unit",
}

local function init()
  storage.deaths = storage.deaths or {}
end

local function research(force)
  local done = 0
  for _, tech in pairs(force.technologies) do
    if tech.researched then
      done = done + 1
    end
  end
  local current = nil
  if force.current_research then
    current = {
      name = force.current_research.name,
      progress = force.research_progress,
    }
  end
  return done, current
end

local function rates(force, surface)
  local out = {}
  local stats = force.get_item_production_statistics(surface)
  for _, item in ipairs(ITEMS) do
    local ok, n = pcall(function()
      return stats.get_flow_count({
        name = item,
        category = "output",
        precision_index = defines.flow_precision_index.one_minute,
      })
    end)
    if ok and type(n) == "number" and n > 0 then
      out[item] = math.floor(n * 10 + 0.5) / 10
    end
  end
  return out
end

local function summary(player)
  local force = player.force
  local done, current = research(force)
  local state = {
    v = 1,
    tick = game.tick,
    played_ticks = game.ticks_played,
    player = player.name,
    researched = done,
    research = current,
    rockets = force.rockets_launched,
    deaths = (storage.deaths and storage.deaths[player.index]) or 0,
  }
  local ok, items = pcall(rates, force, player.surface)
  if ok then
    state.items = items
  end
  return state
end

local function write_all()
  for _, player in pairs(game.connected_players) do
    local ok, state = pcall(summary, player)
    if ok then
      -- for_player: only that player's computer writes the file.
      helpers.write_file(FILE, helpers.table_to_json(state), false, player.index)
    end
  end
end

script.on_init(init)
script.on_configuration_changed(init)

script.on_event(defines.events.on_player_died, function(event)
  init()
  storage.deaths[event.player_index] = (storage.deaths[event.player_index] or 0) + 1
  write_all()
end)

script.on_event(defines.events.on_research_finished, write_all)
script.on_event(defines.events.on_rocket_launched, write_all)
script.on_event(defines.events.on_player_joined_game, write_all)
script.on_nth_tick(EVERY, write_all)
