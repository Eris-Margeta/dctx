# Heredocs and literals stay out of the graph.
module Demo
  def show
    puts <<~EOS, <<-'PLAIN'
      alpha()
    EOS
      beta()
    PLAIN
    puts <<'LIT'
#{not_called()}
LIT
    puts <<"INT"
#{is_called()}
INT
    puts %q{ def hidden_q; end }
    puts %w[gamma delta]
    puts %i[sym_a sym_b]
    puts %{keep #{nested()}}
    ratio = 10 / 2
    pattern = /#{token()}/
    label = :symbol
    other = %s[star]
    joined \
      .tail()
  end
end

=begin
def commented
end
=end

def real
  kept()
end

__END__
def data_only
  ghost()
end
