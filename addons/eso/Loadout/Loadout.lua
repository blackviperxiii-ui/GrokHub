-- Loadout companion addon for The Elder Scrolls Online.
--
-- Records a few facts about your own characters into the LoadoutDB saved
-- variable so the Loadout desktop app can show them: level, champion
-- points, class, race, zone, gold, play time, deaths and achievements. It
-- only listens to ZeniMax's public addon events and reads public API
-- values. It shows nothing, sends no chat, automates nothing and changes
-- nothing in the game.
--
-- ESO writes saved variables to disk only on logout, quit or /reloadui, so
-- that is when Loadout sees new data.

local NAME = "Loadout"
local ADDON_VERSION = "1.0.0"
local MAX_EVENTS = 100

-- Call an API function if this client has it; nil otherwise, never an error.
local function safe(fn, ...)
  if type(fn) ~= "function" then
    return nil
  end
  local ok, v = pcall(fn, ...)
  if ok then
    return v
  end
  return nil
end

local function db()
  LoadoutDB = LoadoutDB or {}
  local d = LoadoutDB
  d.schema = 1
  d.addon = ADDON_VERSION
  d.characters = d.characters or {}
  d.events = d.events or {}
  return d
end

local function charKey()
  local id = safe(GetCurrentCharacterId)
  if id == nil then
    return nil
  end
  return tostring(id)
end

local function char()
  local key = charKey()
  if not key then
    return nil, nil
  end
  local d = db()
  d.characters[key] = d.characters[key] or {}
  return d.characters[key], key
end

local function push(kind, fields)
  local d = db()
  local _, key = char()
  local e = { kind = kind, at = safe(GetTimeStamp), character = key }
  for k, v in pairs(fields or {}) do
    e[k] = v
  end
  table.insert(d.events, e)
  while #d.events > MAX_EVENTS do
    table.remove(d.events, 1)
  end
end

local function snapshot()
  local c = char()
  if not c then
    return
  end
  c.name = safe(GetUnitName, "player")
  c.class = safe(GetUnitClass, "player")
  c.race = safe(GetUnitRace, "player")
  c.level = safe(GetUnitLevel, "player")
  c.champion = safe(GetUnitChampionPoints, "player")
  c.zone = safe(GetPlayerActiveZoneName)
  c.gold = safe(GetCurrentMoney)
  c.played = safe(GetSecondsPlayed)
  c.achievement_points = safe(GetEarnedAchievementPoints)
  c.world = safe(GetWorldName)
  c.seen = safe(GetTimeStamp)
end

local function onDead()
  local c = char()
  if c then
    c.deaths = (c.deaths or 0) + 1
  end
  push("death", { zone = safe(GetPlayerActiveZoneName) })
end

local function onLevel(_, unitTag, level)
  if unitTag ~= "player" then
    return
  end
  snapshot()
  push("level", { level = level })
end

local function onAchievement(_, name, points)
  snapshot()
  push("achievement", { name = name, points = points })
end

local function onLoaded(_, addonName)
  if addonName ~= NAME then
    return
  end
  EVENT_MANAGER:UnregisterForEvent(NAME, EVENT_ADD_ON_LOADED)
  db()
  EVENT_MANAGER:RegisterForEvent(NAME, EVENT_PLAYER_ACTIVATED, snapshot)
  EVENT_MANAGER:RegisterForEvent(NAME, EVENT_ZONE_CHANGED, snapshot)
  EVENT_MANAGER:RegisterForEvent(NAME, EVENT_MONEY_UPDATE, snapshot)
  EVENT_MANAGER:RegisterForEvent(NAME, EVENT_PLAYER_DEAD, onDead)
  EVENT_MANAGER:RegisterForEvent(NAME, EVENT_LEVEL_UPDATE, onLevel)
  EVENT_MANAGER:RegisterForEvent(NAME, EVENT_ACHIEVEMENT_AWARDED, onAchievement)
  EVENT_MANAGER:RegisterForEvent(NAME, EVENT_PLAYER_DEACTIVATED, snapshot)
end

EVENT_MANAGER:RegisterForEvent(NAME, EVENT_ADD_ON_LOADED, onLoaded)
