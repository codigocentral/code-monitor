require 'tempfile'
require 'open3'
require 'bashcov'
require_relative 'bashcov-lexer'

# Test collector correctness independently from production coverage. The old
# greedy lexer incorrectly set line 5's count from the first heredoc command.
file = Tempfile.new(['collector-regression', '.sh'])
file.write("cat << FIRST\nfirst document\nFIRST\nif false; then\n  echo never-executed\nfi\ncat <<'SECOND'\nsecond document\nSECOND\ncase x in\n empty) ;;\nesac\n{\n :\n} > output\n")
file.flush
bash = '/Volumes/LaCie/GitLab/coverage-unitarios-2026-10-17/cache/session-13/bash/bin/bash'
_output, error, syntax = Open3.capture3(bash, '-n', file.path)
raise error unless syntax.success?
hits = [3, nil, nil, 3, nil, nil, 2]
hits[13] = 1
Bashcov::Lexer.new(file.path, hits).complete_coverage
raise 'unexecuted statement was marked covered' unless hits[4] == 0
raise 'spaced heredoc was not recognized' unless hits[1] == 3 && hits[2] == 3
raise 'second heredoc did not retain its own counter' unless hits[7] == 2 && hits[8] == 2
raise 'empty case arm counted as executable' unless hits[10].nil?
raise 'structural closing brace counted as executable' unless hits[14].nil?
puts 'Collector regression passed: documents bounded independently; unexecuted code stays zero.'
file.close!
