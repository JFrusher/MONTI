local function add(a, b) return a + b end

local function sum(a, b)
  return add(a, b)
end

pcall(risky)

local function bump(counter)
  -- increment counter
  counter:increment()
end
