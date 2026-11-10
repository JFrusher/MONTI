local function parse_or(s, fallback)
  local ok, value = pcall(tonumber, s)
  if ok and value then return value end
  return fallback
end
