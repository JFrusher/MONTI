function v = parse_or(s, fallback)
  v = str2double(s);
  if isnan(v)
    v = fallback;
  end
end
