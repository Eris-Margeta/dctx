# Ruby adapter fixture.
require "demo/work"

# Runs a job.
class Box
  def run(value)
    helper(value)
  end
end

def helper(value)
  value
end
