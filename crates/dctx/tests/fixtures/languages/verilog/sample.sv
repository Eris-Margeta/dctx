`define IDENTITY(value) value
module Boxed;
  function automatic integer copy(input integer value);
    copy = `IDENTITY(value);
  endfunction
  task nested;
  endtask
endmodule
