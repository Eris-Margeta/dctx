-module(sample).
-export([copy/1]).
-define(IDENTITY(Value), Value).
copy(Value) ->
    Nested = fun(Item) -> Item end,
    Nested(?IDENTITY(Value)).
helper(Value) -> Value.
café() -> 1.
