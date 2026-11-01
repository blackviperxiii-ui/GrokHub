-- Loadout companion addon for World of Warcraft.
--
-- Records facts about your own characters into the LoadoutDB saved
-- variable so the Loadout desktop app can show them: level, class, spec,
-- zone, gold, item level and gear, professions, currencies, reputations
-- and renown, Mythic+ rating and keystone, Great Vault progress, raid and
-- dungeon lockouts, achievement points, mounts and pets, quests, played
-- time, boss kills and wipes, deaths and achievements.
--
-- It only listens to Blizzard's public addon events and reads public API
-- values. It sends no chat or addon messages, makes no secure calls and
-- automates nothing. It reads nothing in combat: Midnight hides many
-- values then, so Loadout waits until combat ends. It never reads at
-- logout either, when the game has already cleared values such as gold.
--
-- WoW writes saved variables to disk only on logout, exit or /reload, so
-- that is when Loadout sees new data.

local ADDON_VERSION = "1.1.0"
local MAX_EVENTS = 100
local MAX_LIST = 60
-- Seconds to wait after a change before reading, so a burst of events
-- costs one read.
local SETTLE = 2

local f = CreateFrame("Frame")
local dirty = false
local scheduled = false
local greeted = false

-- Retail 12.0 can hand addons "secret" values; those can't be stored, so
-- they are skipped.
local function plain(v)
  if issecretvalue and issecretvalue(v) then
    return nil
  end
  return v
end

-- Calls an API that may not exist in this flavor, and returns its results
-- or nothing.
local function try(fn, ...)
  if type(fn) ~= "function" then
    return nil
  end
  local out = { pcall(fn, ...) }
  if not out[1] then
    return nil
  end
  return unpack(out, 2, table.maxn(out))
end

local function db()
  LoadoutDB = LoadoutDB or {}
  local d = LoadoutDB
  d.schema = 2
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

-- A positive number rounded to one decimal, or nil.
local function round1(v)
  v = plain(v)
  if type(v) ~= "number" or v <= 0 then
    return nil
  end
  return math.floor(v * 10 + 0.5) / 10
end

local SLOTS = {
  [1] = "Head", [2] = "Neck", [3] = "Shoulder", [4] = "Shirt", [5] = "Chest",
  [6] = "Waist", [7] = "Legs", [8] = "Feet", [9] = "Wrist", [10] = "Hands",
  [11] = "Finger", [12] = "Finger", [13] = "Trinket", [14] = "Trinket",
  [15] = "Back", [16] = "Main hand", [17] = "Off hand", [19] = "Tabard",
}

local function gear()
  local out = {}
  local getInfo = (C_Item and C_Item.GetItemInfo) or GetItemInfo
  local getLevel = (C_Item and C_Item.GetDetailedItemLevelInfo) or GetDetailedItemLevelInfo
  for slot = 1, 19 do
    local link = SLOTS[slot] and plain(GetInventoryItemLink("player", slot))
    if link then
      local name, _, quality = try(getInfo, link)
      table.insert(out, {
        slot = SLOTS[slot],
        id = plain(GetInventoryItemID("player", slot)),
        name = plain(name),
        quality = plain(quality),
        level = plain((try(getLevel, link))),
      })
    end
  end
  return out
end

local function professions()
  if not GetProfessions then
    return nil
  end
  local out = {}
  -- Primary, primary, archaeology, fishing, cooking; any can be nil.
  local slots = { try(GetProfessions) }
  for i = 1, 5 do
    if slots[i] then
      local name, _, skill, max = try(GetProfessionInfo, slots[i])
      if plain(name) then
        table.insert(out, { name = plain(name), skill = plain(skill), max = plain(max) })
      end
    end
  end
  return out
end

local function currencies()
  local info = C_CurrencyInfo
  if not (info and info.GetCurrencyListSize and info.GetCurrencyListInfo) then
    return nil
  end
  local out = {}
  for i = 1, (try(info.GetCurrencyListSize) or 0) do
    local c = try(info.GetCurrencyListInfo, i)
    if c and not c.isHeader and plain(c.name) and (plain(c.quantity) or 0) > 0 then
      table.insert(out, { name = plain(c.name), quantity = plain(c.quantity), max = plain(c.maxQuantity) })
      if #out >= MAX_LIST then
        break
      end
    end
  end
  return out
end

local function reputations()
  local out = {}
  local major = C_MajorFactions
  if major and major.GetMajorFactionIDs and major.GetMajorFactionData then
    for _, id in ipairs(try(major.GetMajorFactionIDs) or {}) do
      local m = try(major.GetMajorFactionData, id)
      if m and m.isUnlocked and plain(m.name) then
        table.insert(out, { name = plain(m.name), renown = plain(m.renownLevel) })
      end
    end
  end
  local rep = C_Reputation
  if rep and rep.GetNumFactions and rep.GetFactionDataByIndex then
    for i = 1, (try(rep.GetNumFactions) or 0) do
      local r = try(rep.GetFactionDataByIndex, i)
      if #out >= MAX_LIST then
        break
      end
      if r and not r.isHeader and plain(r.name) then
        table.insert(out, {
          name = plain(r.name),
          standing = plain(r.reaction),
          value = plain(r.currentStanding),
          low = plain(r.currentReactionThreshold),
          high = plain(r.nextReactionThreshold),
        })
      end
    end
  end
  return out
end

local function mythicPlus(c)
  local cm = C_ChallengeMode
  c.mplusScore = round1(try(cm and cm.GetOverallDungeonScore)) or c.mplusScore
  local mp = C_MythicPlus
  local level = plain(try(mp and mp.GetOwnedKeystoneLevel))
  local map = plain(try(mp and mp.GetOwnedKeystoneChallengeMapID))
  if level and map then
    c.keystone = { level = level, map = map, name = plain((try(cm and cm.GetMapUIInfo, map))) }
  elseif mp and mp.GetOwnedKeystoneLevel then
    c.keystone = nil
  end
end

local function vault()
  local wr = C_WeeklyRewards
  if not (wr and wr.GetActivities) then
    return nil
  end
  local out = {}
  for _, a in ipairs(try(wr.GetActivities) or {}) do
    table.insert(out, {
      kind = plain(a.type),
      index = plain(a.index),
      progress = plain(a.progress),
      threshold = plain(a.threshold),
      level = plain(a.level),
    })
  end
  return out
end

local function lockouts()
  if not (GetNumSavedInstances and GetSavedInstanceInfo) then
    return nil
  end
  local out = {}
  for i = 1, (try(GetNumSavedInstances) or 0) do
    local name, _, reset, _, locked, _, _, isRaid, _, difficulty, total, done = try(GetSavedInstanceInfo, i)
    if plain(name) and locked then
      reset = plain(reset)
      table.insert(out, {
        name = plain(name),
        raid = plain(isRaid) and true or false,
        difficulty = plain(difficulty),
        done = plain(done),
        total = plain(total),
        resetAt = reset and (time() + reset) or nil,
      })
    end
  end
  return out
end

local function collections(c)
  local mj = C_MountJournal
  if mj and mj.GetMountIDs and mj.GetMountInfoByID then
    local owned = 0
    for _, id in ipairs(try(mj.GetMountIDs) or {}) do
      if select(11, try(mj.GetMountInfoByID, id)) then
        owned = owned + 1
      end
    end
    c.mounts = owned
  end
  local pj = C_PetJournal
  if pj and pj.GetNumPets then
    local _, owned = try(pj.GetNumPets)
    c.pets = plain(owned) or c.pets
  end
  c.achievementPoints = plain((try(GetTotalAchievementPoints))) or c.achievementPoints
end

local function quests(c)
  local ql = C_QuestLog
  if ql and ql.GetNumQuestLogEntries then
    local _, count = try(ql.GetNumQuestLogEntries)
    c.questsInLog = plain(count)
  end
  if ql and ql.GetAllCompletedQuestIDs then
    local done = try(ql.GetAllCompletedQuestIDs)
    if type(done) == "table" then
      c.questsCompleted = #done
    end
  end
end

-- Reads everything about the logged-in character. Never in combat, where
-- Midnight hides values; it runs once combat ends instead.
local function snapshot()
  if InCombatLockdown and InCombatLockdown() then
    dirty = true
    return
  end
  dirty = false
  local c = char()
  if not c then
    return
  end
  c.name = plain(UnitName("player"))
  c.realm = plain(GetRealmName())
  local className, classFile = UnitClass("player")
  c.class = plain(classFile)
  c.className = plain(className)
  c.race = plain((UnitRace("player")))
  c.faction = plain((UnitFactionGroup("player")))
  c.level = plain(UnitLevel("player")) or c.level
  c.zone = plain(GetRealZoneText()) or c.zone
  c.money = plain(GetMoney()) or c.money
  local overall, equipped = try(GetAverageItemLevel)
  -- Item data can still be loading at login and reads as 0; keep the last
  -- real value then.
  c.itemLevel = round1(equipped) or c.itemLevel
  c.itemLevelOverall = round1(overall) or c.itemLevelOverall
  local spec = plain(try(GetSpecialization))
  if spec then
    local _, specName, _, _, role = try(GetSpecializationInfo, spec)
    c.spec = plain(specName) or c.spec
    c.role = plain(role) or c.role
  end
  if IsInGuild and IsInGuild() then
    c.guild = plain((GetGuildInfo("player")))
  else
    c.guild = nil
  end
  c.gear = gear()
  c.professions = professions()
  c.currencies = currencies()
  c.reputations = reputations()
  mythicPlus(c)
  c.vault = vault()
  c.lockouts = lockouts()
  collections(c)
  quests(c)
  c.lastSeen = time()
  c.build = (select(4, GetBuildInfo()))
end

-- Reads once things settle, so a burst of events costs one read.
local function queue()
  dirty = true
  if scheduled then
    return
  end
  if not (C_Timer and C_Timer.After) then
    snapshot()
    return
  end
  scheduled = true
  C_Timer.After(SETTLE, function()
    scheduled = false
    if dirty then
      snapshot()
    end
  end)
end

local function summary()
  local c = char()
  if not c then
    return
  end
  local who = (c.level or "?") .. " " .. (c.spec and (c.spec .. " ") or "") .. (c.className or "")
  local parts = { c.name or "", who }
  if c.itemLevel then
    table.insert(parts, "ilvl " .. c.itemLevel)
  end
  if c.money then
    table.insert(parts, math.floor(c.money / 10000) .. "g")
  end
  if c.mplusScore then
    table.insert(parts, "M+ " .. c.mplusScore)
  end
  if c.keystone and c.keystone.level then
    table.insert(parts, "key +" .. c.keystone.level .. " " .. (c.keystone.name or ""))
  end
  print("|cffffb000Loadout|r " .. ADDON_VERSION .. ": " .. table.concat(parts, " · "))
  print("|cffffb000Loadout|r saves this for the Loadout app when you /reload or log out.")
end

-- Clicking Loadout in the minimap's addon menu prints the summary.
function Loadout_OnAddonCompartmentClick()
  snapshot()
  summary()
end

f:RegisterEvent("PLAYER_LOGIN")
f:RegisterEvent("PLAYER_ENTERING_WORLD")
f:RegisterEvent("PLAYER_REGEN_ENABLED")
f:RegisterEvent("PLAYER_LEVEL_UP")
f:RegisterEvent("PLAYER_MONEY")
f:RegisterEvent("ZONE_CHANGED_NEW_AREA")
f:RegisterEvent("PLAYER_EQUIPMENT_CHANGED")
f:RegisterEvent("TIME_PLAYED_MSG")
f:RegisterEvent("ENCOUNTER_END")
f:RegisterEvent("PLAYER_DEAD")
-- Not every flavor has these.
for _, event in ipairs({
  "ACHIEVEMENT_EARNED",
  "PLAYER_SPECIALIZATION_CHANGED",
  "UPDATE_INSTANCE_INFO",
  "CHALLENGE_MODE_COMPLETED",
  "CHALLENGE_MODE_MAPS_UPDATE",
  "WEEKLY_REWARDS_UPDATE",
  "MAJOR_FACTION_RENOWN_LEVEL_CHANGED",
  "CURRENCY_DISPLAY_UPDATE",
  "SKILL_LINES_CHANGED",
}) do
  pcall(f.RegisterEvent, f, event)
end

f:SetScript("OnEvent", function(_, event, ...)
  if event == "PLAYER_LOGIN" then
    -- Ask the game for data it sends later, through the events above.
    try(RequestRaidInfo)
    try(C_MythicPlus and C_MythicPlus.RequestMapInfo)
    if C_Timer and C_Timer.After then
      C_Timer.After(8, function()
        try(RequestTimePlayed)
      end)
    end
    queue()
  elseif event == "PLAYER_ENTERING_WORLD" then
    queue()
    if not greeted then
      greeted = true
      print("|cffffb000Loadout|r " .. ADDON_VERSION .. " is recording for the Loadout app. Type /loadout for a summary.")
    end
  elseif event == "PLAYER_REGEN_ENABLED" then
    if dirty then
      queue()
    end
  elseif event == "PLAYER_LEVEL_UP" then
    local level = ...
    push("level", { level = level })
    queue()
  elseif event == "TIME_PLAYED_MSG" then
    local total, thisLevel = ...
    local c = char()
    if c then
      c.played = plain(total) or c.played
      c.playedLevel = plain(thisLevel) or c.playedLevel
    end
  elseif event == "ENCOUNTER_END" then
    local id, name, difficulty, size, success = ...
    push(success == 1 and "kill" or "wipe", {
      encounter = id,
      name = name,
      difficulty = difficulty,
      size = size,
    })
    dirty = true
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
    queue()
  else
    queue()
  end
end)

SLASH_LOADOUT1 = "/loadout"
SlashCmdList.LOADOUT = function()
  snapshot()
  summary()
end
