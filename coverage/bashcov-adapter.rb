# Bashcov 1.8.2's CLI prepends bash_path and caches /bin/bash's version before
# parsing --bash-path. Mirror its launcher, deriving that version from the
# selected executable before tracing. Coverage collection/lexer are unchanged.
require 'bashcov'
require_relative 'bashcov-lexer'
Bashcov.parse_options! ARGV
version = IO.popen([Bashcov.bash_path, '-c', 'printf "%s.%s" "${BASH_VERSINFO[0]}" "${BASH_VERSINFO[1]}"'], &:read)
abort 'Unable to determine selected Bash version' unless version.match?(/\A\d+\.\d+\z/)
Bashcov.send(:remove_const, :BASH_VERSION)
Bashcov.const_set(:BASH_VERSION, version.freeze)
runner = Bashcov::Runner.new(Bashcov.command)
run_status = runner.run
coverage_data = runner.result
require 'simplecov'
SimpleCov.start
SimpleCov.command_name Bashcov.command_name
SimpleCov.root Bashcov.root_directory
result = SimpleCov::Result.new(coverage_data)
if SimpleCov.use_merging
  SimpleCov::ResultMerger.store_result(result)
  result = SimpleCov::ResultMerger.merged_result
end
SimpleCov.at_exit do
  puts "Run completed using #{Bashcov.fullname}"
  result.format!
end
exit run_status.exitstatus
