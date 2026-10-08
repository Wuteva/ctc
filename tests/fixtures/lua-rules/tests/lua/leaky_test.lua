-- A test that leaks a global and reads the clock of the host.
local leaky = {}

function leaky.run()
  seed = os.time()
  return seed
end

return leaky
