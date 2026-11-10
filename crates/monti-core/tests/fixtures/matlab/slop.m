function load()
  try
    parse();
  catch
  end
  try
    parse();
  catch
    % ignore
  end
end

function bump(counter)
  % increment counter
  counter.increment();
end
