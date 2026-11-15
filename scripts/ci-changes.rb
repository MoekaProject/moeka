#!/usr/bin/env ruby
require 'open3'

module CiChanges
  module_function

  def native?(paths)
    paths.any? do |path|
      !path.match?(%r{\A(?:[^/]+\.md|Casks/[^/]+\.rb|packaging/.*\.md|\.github/[^/]+\.md|\.github/ISSUE_TEMPLATE/[^/]+\.yml)\z})
    end
  end

  def changed_paths(base)
    return nil unless base.match?(/\A[0-9a-f]{40}\z/) && base != '0' * 40

    output, status = Open3.capture2e('git', 'diff', '--name-only', '-z', base, 'HEAD', '--')
    unless status.success?
      warn "Could not determine changed paths; running all native checks: #{output}"
      return nil
    end
    output.split("\0")
  end
end

if $PROGRAM_NAME == __FILE__
  paths = CiChanges.changed_paths(ENV.fetch('BASE_SHA', ''))
  warn 'No usable change base; running all native checks.' if paths.nil?
  puts "native=#{paths.nil? || CiChanges.native?(paths)}"
end
