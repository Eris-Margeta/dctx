type 'a boxed = { value : 'a }
let copy value =
  let nested item = item in
  nested value
let café () = 1
