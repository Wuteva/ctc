-- A module in Lua 5.5 syntax. The collective declaration binds every free
-- name, so the global rules accept the names that the package state gives.
global<const> *

local strict = {}

function strict.next(value)
  return value + 1, tostring(value)
end

function strict.greet(name)
  print("hello " .. name)
end

return strict
