-- Loadout companion addon for World of Warcraft.
--
-- Records a few facts about your own characters into the LoadoutDB saved
-- variable so the Loadout desktop app can show them: level, class, zone,
-- gold, item level, played time (when you type /played), boss kills and
-- wipes, deaths and achievements. It only listens to Blizzard's public
-- addon events and reads public API values. It shows no frames, sends no
-- chat or addon messages, makes no secure calls and automates nothing.
--
-- WoW writes saved variables to disk only on logout, exit or /reload, so
-- that is when Loadout sees new data.

local ADDON_VERSION = "1.0.0"
local MAX_EVENTS = 100

local f = CreateFrame("Frame")

-- Retail 12.0 can hand addons "secret" values during combat; those can't be
-- stored, so they are skipped.
local function plain(v)
  if issecretvalue and issecretvalue(v) then
    return nil
  end
  return v
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
  local name = plain(UnitName("player"))
  local realm = plain(GetRealmName())
  if not name or not realm then
    return nil
  end
  return name .. "-" .. realm
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
  local e = { kind = kind, at = time(), character = key }
  for k, v in pairs(fields or {}) do
    e[k] = plain(v)
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
  c.name = plain(UnitName("player"))
  c.realm = plain(GetRealmName())
  local _, classFile = UnitClass("player")
  c.class = plain(classFile)
  c.className = plain((UnitClass("player")))
  c.race = plain((UnitRace("player")))
  c.faction = plain((UnitFactionGroup("player")))
  c.level = plain(UnitLevel("player"))
  c.zone = plain(GetRealZoneText())
  c.money = plain(GetMoney())
  if GetAverageItemLevel then
    local _, equipped = GetAverageItemLevel()
    if equipped then
      c.itemLevel = math.floor(equipped * 10 + 0.5) / 10
    end
  end
  if IsInGuild and IsInGuild() then
    c.guild = plain((GetGuildInfo("player")))
  else
    c.guild = nil
  end
  c.lastSeen = time()
  c.build = (select(4, GetBuildInfo()))
end

f:RegisterEvent("PLAYER_LOGIN")
f:RegisterEvent("PLAYER_ENTERING_WORLD")
f:RegisterEvent("PLAYER_LEVEL_UP")
f:RegisterEvent("PLAYER_MONEY")
f:RegisterEvent("ZONE_CHANGED_NEW_AREA")
f:RegisterEvent("PLAYER_EQUIPMENT_CHANGED")
f:RegisterEvent("TIME_PLAYED_MSG")
f:RegisterEvent("ENCOUNTER_END")
f:RegisterEvent("PLAYER_DEAD")
f:RegisterEvent("PLAYER_LOGOUT")
-- Not every flavor has achievements.
pcall(f.RegisterEvent, f, "ACHIEVEMENT_EARNED")

f:SetScript("OnEvent", function(_, event, ...)
  if event == "PLAYER_LEVEL_UP" then
    local level = ...
    push("level", { level = level })
    snapshot()
  elseif event == "TIME_PLAYED_MSG" then
    local total, thisLevel = ...
    local c = char()
    if c then
      c.played = plain(total)
      c.playedLevel = plain(thisLevel)
    end
  elseif event == "ENCOUNTER_END" then
    local id, name, difficulty, size, success = ...
    push(success == 1 and "kill" or "wipe", {
      encounter = id,
      name = name,
      difficulty = difficulty,
      size = size,
    })
  elseif event == "PLAYER_DEAD" then
    local c = char()
    if c then
      c.deaths = (c.deaths or 0) + 1
    end
    push("death", { zone = GetRealZoneText() })
  elseif event == "ACHIEVEMENT_EARNED" then
    local id = ...
    local name
    if GetAchievementInfo then
      name = select(2, GetAchievementInfo(id))
    end
    push("achievement", { id = id, name = name })
  else
    snapshot()
  end
end)

SLASH_LOADOUT1 = "/loadout"
SlashCmdList.LOADOUT = function()
  print("|cffffb000Loadout|r " .. ADDON_VERSION .. ": recording your characters for the Loadout app. Type /reload or log out to save now.")
end
