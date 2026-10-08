-- Unrestricted loading: code loading, files, programs, the debug library,
-- and a module name that is not a literal.
local loading = {}

function loading.run(text, name)
  local chunk = load(text)
  local other = _G.dofile(name)
  local alias = load
  pcall(loadfile, name)
  local stamp = os.time()
  local file = io.open(name)
  local bytes = string.dump(chunk)
  local trace = debug.traceback()
  package.loaded[name] = nil
  local used = collectgarbage("count")
  local module = require(name)
  local key = _G[name]
  local lookup = string[name]
  return chunk, other, alias, stamp, file, bytes, trace, used, module, key, lookup
end

function loading.allowed(name)
  local function load(value)
    return value
  end
  local part = require("app.parts")
  return load(name), part, string.format("%s", name), string.rep("x", 2)
end

return loading
