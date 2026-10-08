-- Calls of require that the package rule refuses.
local bad = {}

local Dkjson = require("dkjson")
local Parts = require("arena.parts")
local Dynamic = require(Parts.name)

function bad.run()
  return Dkjson, Dynamic
end

return bad
