local Boxed = {}
function Boxed.copy(value)
  local function nested(item)
    return item
  end
  return nested(value)
end
return Boxed
