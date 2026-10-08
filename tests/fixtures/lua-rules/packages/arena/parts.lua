-- Parts of the arena robots. This file follows every rule.
local Shapes = require("app.shapes")

local parts = {}

local DEFAULT_SCALE = 1

local function total(...)
  local sum = 0
  for _, value in ipairs({ ... }) do
    sum = sum + value
  end
  return sum
end

function parts.mass(part)
  local scale = part.scale or DEFAULT_SCALE
  return total(part.base, scale * Shapes.mass(part.shape))
end

function parts:describe()
  return string.format("%s (%d)", self.name, parts.mass(self))
end

parts.kinds = { wheel = "drive", flipper = "weapon" }

return parts
