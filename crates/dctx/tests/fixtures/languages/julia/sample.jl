struct Boxed{T}
    value::T
end
function copy(value::T) where T
    function nested(item)
        item
    end
    nested(value)
end
macro identity(value)
    esc(value)
end
café() = 1
