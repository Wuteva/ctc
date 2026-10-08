-- A behavior test in the form of tests/lua. The test runner gives describe,
-- it, and expect.
local parts = require("arena.parts")

describe("parts", function()
  it("adds the mass", function()
    expect(parts.mass({ base = 1, scale = 2 })).toBe(3)
  end)
end)
