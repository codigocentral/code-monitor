require 'json'
class UnitLcovFormatter
  def format(result)
    files = {}
    File.open(File.join(SimpleCov.coverage_path, 'coverage.info'), 'w') do |out|
      result.files.each do |file|
        files[file.filename] = file.coverage
        out.puts "SF:#{file.filename}"
        values = file.coverage.each_with_index.reject { |hits, _index| hits.nil? }
        values.each { |hits, index| out.puts "DA:#{index + 1},#{hits}" }
        out.puts "LF:#{values.size}"
        out.puts "LH:#{values.count { |hits, _index| hits > 0 }}"
        out.puts 'end_of_record'
      end
    end
    File.write(File.join(SimpleCov.coverage_path, 'raw-lines.json'), JSON.pretty_generate({tool: 'bashcov 1.8.2 / SimpleCov 0.15.1', supported_metrics: ['lines'], unsupported_metrics: ['branches', 'functions'], files: files}))
  end
end
production = %w[generate-certs.sh install.sh scripts/enable-mariadb.sh scripts/enable-postgres.sh scripts/install-remote.sh scripts/publish-to-github.sh service/linux/install.sh service/linux/uninstall.sh service/macos/install.sh service/macos/uninstall.sh].map { |path| File.expand_path(path, SimpleCov.root) }
SimpleCov.coverage_dir(ENV.fetch('UNIT_SHELL_OUTPUT'))
SimpleCov.add_filter { |file| !production.include?(file.filename) }
SimpleCov.formatter = UnitLcovFormatter
