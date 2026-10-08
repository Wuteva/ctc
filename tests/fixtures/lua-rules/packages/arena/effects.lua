-- Work at the top level of a module: a call, a condition, a loop, a name
-- that is assigned, and a global function.
local effects = {}

print("loading")

effects.ready = true

if effects.ready then
  effects.mode = "fast"
end

counter = 0

function register()
  return effects
end

for index = 1, 3 do
  effects[index] = index
end

return effects
