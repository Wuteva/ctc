-- Accidental globals: a read of a name that is not declared, a name that
-- is used before its local declaration, and assignments to globals.
local accidents = {}

local function first()
  return second()
end

local function second()
  total = total + 1
  return total
end

function accidents.reset()
  score = 0
  _G.cache = {}
  _G[score] = 1
end

function accidents.run()
  return first() + second() + math.floor(1.5)
end

return accidents
