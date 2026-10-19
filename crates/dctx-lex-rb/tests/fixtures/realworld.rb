# Real-world Ruby constructs that must parse completely.
# Spec PARSERS-v1: heredocs, modifiers, globals, and character literals.
# Depends on Ruby syntax only; used by tests/dctx_lex_rb.rs.
# Endless methods, singleton class, and end-while stay balanced.

class Certifi < Formula
  class <<self
    def metaclass_hook
    end
  end

  def pythons
    deps.filter_map { |dep| dep.to_formula if dep.name.start_with?("python@") }
  end

  def install
    oldest, python = pythons.sort_by(&:version)
    on_macos do
      depends_on "llvm" if DevelopmentTools.clang_build_version <= 1500
    end
    python.versioned_formulae.each do |extra|
      next if extra.version < oldest.version
      site_packages.find do |path|
        path.install_symlink path if path.file?
      end
    end
  end

  test do
    return unless OS.mac?
    output = shell_output("#{bin}/tool")
    args = %W[
      -DRPATH=#{rpath}
    ]
    (testpath/"t.cpp").write <<~CPP
      int main() {}
    CPP
    output
  end
end

def square(n) = n * n

def tagged(value) = value.class

def anchor
end

def show
  puts <<~EOS.chomp, <<-'PLAIN'
    alpha()
  EOS
    beta()
  PLAIN
  joined \
    .tail
end

def spin
  begin
    work
  end while ready
  read rescue begin
    fallback
  end
end

def wrapped
  value = (begin
    risky
  rescue
    nil
  end)
end

def wait_loop
  until ready
    pause
  end
  while ready
    pause
  end
  unless ready
    pause
  end
end

def classify(value)
  case value
  in 1
    one
  in [2, 3]
    many
  in 4 if value.positive?
    guarded
  else
    other
  end
end

def touch(obj)
  obj&.save
end

def punct
  post = $'
  loaded = $"
  pre = $`
  match = $~
  group = $1
  mark = ?x
  quote = ?"
  slash = ?/
  ratio %= 2
  arrow = ->(x) { x }
  stab = ->(x) do
    x.succ
  end
end
