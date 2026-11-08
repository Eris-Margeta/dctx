module sample
contains
  integer function copy(value) result(output)
    integer, intent(in) :: value
    output = nested(value)
  contains
    integer function nested(item)
      integer, intent(in) :: item
      nested = item
    end function nested
  end function copy
end module sample
