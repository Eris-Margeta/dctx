defmodule Sample.Boxed do
  @moduledoc "Fixture"
  defmacro identity(value) do
    quote do: unquote(value)
  end
  @spec copy(term()) :: term()
  def copy(value) do
    nested = fn item -> item end
    nested.(value)
  end
  def café(), do: 1
end
