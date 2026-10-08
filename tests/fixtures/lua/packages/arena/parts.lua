-- Parts of the arena robots. This file follows every rule.
local Parts = {}

local Shapes = require("app.shapes")

local function total(...values)
  local sum = 0
  for i = 1, values.n do
    sum = sum + values[i]
  end
  return sum
end

function Parts.mass(part)
  local<const> base, scale = 2, part.scale or 1
  return total(base, scale, Shapes.mass(part.shape))
end

function Parts:describe()
  return string.format("%s (%d)", self.name, Parts.mass(self))
end

return Parts
