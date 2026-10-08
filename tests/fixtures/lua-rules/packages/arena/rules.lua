-- An entry point in the form of the core package: several local module
-- tables and one table that maps module names to them. This file follows
-- every rule.
local entry = {}

entry.id = "arena"

local rules = {}

-- The last countdown second that this match announced.
local lastCountdown = nil

local function countdown(ctx)
  local seconds = math.floor(ctx.match.timeLeftS + 0.5)
  if seconds >= 1 and seconds ~= lastCountdown then
    lastCountdown = seconds
    ctx:event("countdown", { severity = 1 })
  end
end

function rules.on_phase(_ctx, phase)
  if phase == "active" then
    lastCountdown = nil
  end
end

function rules.post_step(ctx)
  countdown(ctx)
end

local damage = {}

function damage.on_impacts(ctx, batch)
  for _, impact in ipairs(batch) do
    ctx:damage(impact.a, { hp = impact.normalSpeed })
  end
end

entry.modules = {
  ["arena.rules.standard"] = rules,
  ["arena.damage.standard"] = damage,
}

return entry
