#!/usr/bin/env ruby
require 'minitest/autorun'
require 'tmpdir'
require 'open3'
require_relative 'remote-artifact'

class RemoteArtifactTest < Minitest::Test
  def test_notarization_failure_never_publishes_a_signed_revision_or_retries
    root = File.expand_path('..', __dir__)
    app_version = File.read(File.join(root, 'Cargo.toml'))[/^version = "([^"]+)"/, 1]
    server_version = File.read(File.join(root, 'crates/moeka-remote-server/Cargo.toml'))[/^version = "([^"]+)"/, 1]
    Dir.mktmpdir do |directory|
      tools = File.join(directory, 'tools')
      Dir.mkdir(tools)
      {
        'codesign' => 'echo codesign >> "$CALLS"; if [[ "$1" != --verify ]]; then printf signature >> "${@: -1}"; fi',
        'ditto' => 'touch "${@: -1}"',
        'xcrun' => 'if [[ "$1 ${2:-}" == "notarytool submit" ]]; then echo submit >> "$CALLS"; printf \'{"id":"fixture","status":"%s"}\n\' "$STATUS"; fi'
      }.each do |name, body|
        path = File.join(tools, name)
        File.write(path, "#!/usr/bin/env bash\nset -euo pipefail\n#{body}\n")
        File.chmod(0o700, path)
      end
      %w[Accepted Rejected].each do |status|
        payload = File.join(directory, status)
        Dir.mkdir(payload)
        target = 'macos-aarch64'
        binary = File.join(payload, "#{target}.bin")
        manifest = File.join(payload, "#{target}.json")
        File.binwrite(binary, [0xfeedfacf, 0x0100000c].pack('V2') + 'unsigned')
        metadata = { 'build' => { 'server_version' => server_version, 'app_version' => app_version, 'source_sha' => 'a' * 40 }, 'target' => target }
        File.write(manifest, JSON.generate(metadata))
        args = [payload, app_version, server_version, 'a' * 40, target]
        remote_artifact('record', *args)
        before = File.read(manifest)
        calls = File.join(payload, 'calls')
        environment = {
          'PATH' => "#{tools}:#{ENV.fetch('PATH')}",
          'GITHUB_ACTIONS' => 'true', 'RUNNER_ENVIRONMENT' => 'github-hosted',
          'DEVELOPER_ID_APPLICATION' => 'Fixture identity',
          'SIGNING_KEYCHAIN' => File.join(payload, 'fixture-keychain'),
          'NOTARY_OUTPUT_DIR' => File.join(payload, 'recovery'),
          'MOEKA_REMOTE_SOURCE_SHA' => 'a' * 40, 'CALLS' => calls, 'STATUS' => status
        }
        output, error, result = Open3.capture3(environment, 'bash', 'scripts/sign-remote-server.sh', payload, target, chdir: root)
        assert_equal(status == 'Accepted', result.success?, output + error)
        assert_equal 1, File.readlines(calls).count { |line| line.strip == 'submit' }
        if status == 'Accepted'
          remote_artifact('verify', *args)
        else
          assert_equal before, File.read(manifest)
          assert File.file?(File.join(payload, 'recovery', "#{target}.json"))
        end
      end
    end
  end

  def test_signed_bytes_preserve_build_identity_and_require_a_new_digest
    Dir.mktmpdir do |directory|
      target = 'macos-aarch64'
      args = [directory, '0.1.38', '0.1.0', 'a' * 40, target]
      binary = File.join(directory, "#{target}.bin")
      manifest = File.join(directory, "#{target}.json")
      File.binwrite(binary, [0xfeedfacf, 0x0100000c].pack('V2') + 'unsigned')
      metadata = {
        'build' => { 'server_version' => args[2], 'app_version' => args[1], 'source_sha' => args[3] },
        'target' => target
      }
      File.write(manifest, JSON.generate(metadata))
      remote_artifact('record', *args)
      remote_artifact('verify', *args)
      File.open(binary, 'ab') { |file| file.write('signature') }
      assert_raises(RuntimeError) { remote_artifact('verify', *args) }
      remote_artifact('record', *args)
      remote_artifact('verify', *args)
      wrong_source = args.dup
      wrong_source[3] = 'b' * 40
      assert_raises(RuntimeError) { remote_artifact('record', *wrong_source) }
      wrong_arch = args.dup
      wrong_arch[4] = 'macos-x86_64'
      File.binwrite(File.join(directory, 'macos-x86_64.bin'), File.binread(binary))
      File.write(File.join(directory, 'macos-x86_64.json'), JSON.generate(metadata.merge('target' => wrong_arch[4])))
      assert_raises(RuntimeError) { remote_artifact('record', *wrong_arch) }
      File.unlink(binary)
      File.symlink(manifest, binary)
      assert_raises(RuntimeError) { remote_artifact('record', *args) }
    end
  end
end
