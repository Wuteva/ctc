local Loader = {}

local Json = require("dkjson")

function Loader.run(text)
  local chunk = load(text)
  return chunk()
end

function Loader.include(path)
  return _G.dofile(path)
end

function Loader.decode(text)
  local result = {}
  local value = Json.decode(text)
  for key, item in pairs(value) do
    if type(item) == "table" then
      result[key] = item
    else
      result[key] = { item }
    end
  end
  setmetatable(result, { __index = Loader })
  return result
end

return Loader
