def load
  begin
    parse
  rescue StandardError
  end
  begin
    parse
  rescue StandardError
    # ignore
  end
end

def sum(a, b)
  add(a, b)
end

def bump(counter)
  # increment counter
  counter.increment
end
