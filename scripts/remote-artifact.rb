#!/usr/bin/env ruby
# Validate unsigned provenance before signing, then record the signed bytes.
require 'digest'
require 'json'

def remote_artifact(mode, directory, app_version, server_version, source_sha, target)
  raise 'Invalid operation' unless %w[verify record].include?(mode)
  raise 'Invalid source identity' unless source_sha.match?(/\A[0-9a-f]{40}\z/)
  cpu = { 'macos-x86_64' => 0x01000007, 'macos-aarch64' => 0x0100000c }.fetch(target)
  binary = File.join(directory, "#{target}.bin")
  manifest = File.join(directory, "#{target}.json")
  [binary, manifest].each do |path|
    raise "Not a regular artifact: #{path}" unless File.lstat(path).file? && !File.symlink?(path)
  end
  raise 'Unexpected Mach-O architecture' unless File.binread(binary, 8).unpack('V2') == [0xfeedfacf, cpu]
  identity = {
    'server_version' => server_version,
    'app_version' => app_version,
    'source_sha' => source_sha
  }
  metadata = JSON.parse(File.read(manifest))
  raise 'Artifact source/build mismatch' unless metadata['build'] == identity && metadata['target'] == target
  revision = { 'size' => File.size(binary), 'sha256' => Digest::SHA256.file(binary).digest.bytes }
  if mode == 'verify'
    raise 'Artifact integrity mismatch' unless metadata['revision'] == revision
  else
    metadata['revision'] = revision
    File.write(manifest, JSON.generate(metadata) + "\n")
  end
end

if $PROGRAM_NAME == __FILE__
  abort "Usage: #{$PROGRAM_NAME} verify|record DIRECTORY APP_VERSION SERVER_VERSION SOURCE_SHA TARGET" unless ARGV.length == 6
  remote_artifact(*ARGV)
end
