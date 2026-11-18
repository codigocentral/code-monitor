# Bashcov 1.8.2's greedy heredoc regexp consumes everything up to the LAST
# delimiter in a file, assigning the first command's hits to unexecuted code
# between documents. Bound the match at the FIRST complete delimiter line;
# also accept the valid whitespace after << used by generate-certs.sh.
require 'bashcov/lexer'
class Bashcov::Lexer
  alias_method :original_mark_multiline, :mark_multiline
  alias_method :original_relevant?, :relevant?
  def relevant?(line)
    # These are structural syntax, without an executable command to trace:
    # an empty case arm, and the closing brace of a redirected group.
    return false if line.match?(/\A\s*[^)]+\)\s*;;\s*\z/) || line.match?(/\A\s*}\s*(?:[<>]|\d+>)/)
    original_relevant?(line)
  end
  def mark_multiline(lines, lineno, regexp, forward: true)
    if regexp.source.include?('<<-?')
      regexp = /\A[^\n]+<<-?\s*['"]?(\w+)['"]?[^\n]*\n.*?^\s*\1\s*$/m
    end
    original_mark_multiline(lines, lineno, regexp, forward: forward)
  end
end
