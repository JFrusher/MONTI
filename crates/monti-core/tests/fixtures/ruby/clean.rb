def parse_or(s, fallback)
  Integer(s)
rescue ArgumentError
  fallback
end
