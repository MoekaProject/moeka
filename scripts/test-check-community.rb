#!/usr/bin/env ruby
require 'tmpdir'
require_relative 'check-community'
require_relative 'ci-changes'
require 'json'

class CommunityChecksTest
  def assert_empty(value)
    raise "Expected an empty result, got #{value.inspect}" unless value.empty?
  end

  def refute_empty(value)
    raise 'Expected validation errors' if value.empty?
  end

  def assert_equal(expected, actual)
    raise "Expected #{expected.inspect}, got #{actual.inspect}" unless expected == actual
  end

  def assert_raises(type)
    begin
      yield
    rescue type
      return
    end
    raise "Expected #{type} to be raised"
  end

  def form
    {
      'name' => 'Bug report', 'description' => 'Report a source bug',
      'body' => [
        { 'type' => 'markdown', 'attributes' => { 'value' => 'Do not share private data.' } },
        { 'type' => 'textarea', 'id' => 'steps', 'attributes' => { 'label' => 'Steps' },
          'validations' => { 'required' => true } }
      ]
    }
  end

  def workflow
    {
      true => { 'pull_request' => nil },
      'permissions' => { 'contents' => 'read' },
      'jobs' => { 'check' => { 'steps' => [
        { 'uses' => "actions/checkout@#{'a' * 40}", 'with' => { 'persist-credentials' => false } }
      ] } }
    }
  end

  def test_forms_reject_duplicate_ids_and_invalid_validation
    assert_empty CommunityChecks.issue_form(form, 'test')
    duplicate = form
    duplicate['body'] << duplicate['body'].last.dup
    refute_empty CommunityChecks.issue_form(duplicate, 'test')
    invalid = form
    invalid['body'].last['validations']['required'] = 'true'
    refute_empty CommunityChecks.issue_form(invalid, 'test')
  end

  def test_forms_validate_dropdowns_and_checkboxes
    document = form
    document['body'] << { 'type' => 'dropdown', 'id' => 'os',
                          'attributes' => { 'label' => 'OS', 'options' => ['macOS', 'Linux'] } }
    assert_empty CommunityChecks.issue_form(document, 'test')
    document['body'].last['attributes']['options'] = []
    refute_empty CommunityChecks.issue_form(document, 'test')
    document['body'][-1] = { 'type' => 'checkboxes', 'id' => 'privacy',
                             'attributes' => { 'label' => 'Privacy',
                                               'options' => [{ 'label' => 'Redacted', 'required' => true }] } }
    assert_empty CommunityChecks.issue_form(document, 'test')
  end

  def test_workflows_preserve_fork_pr_boundaries
    assert_empty CommunityChecks.workflow(workflow, 'test')
    unsafe = workflow
    unsafe[true] = { 'pull_request_target' => nil }
    refute_empty CommunityChecks.workflow(unsafe, 'test')
    unsafe = workflow
    unsafe['jobs']['check']['permissions'] = { 'contents' => 'write' }
    refute_empty CommunityChecks.workflow(unsafe, 'test')
    unsafe = workflow
    unsafe['jobs']['check']['steps'][0]['uses'] = 'actions/checkout@v4'
    refute_empty CommunityChecks.workflow(unsafe, 'test')
    unsafe = workflow
    unsafe['jobs']['check']['steps'][0]['with'].delete('persist-credentials')
    refute_empty CommunityChecks.workflow(unsafe, 'test')
  end

  def test_signed_candidates_keep_credentials_out_of_builds_and_never_publish
    path = File.expand_path('../.github/workflows/signed-macos.yml', __dir__)
    document = CommunityChecks.yaml_file(path)
    assert_empty CommunityChecks.workflow(document, path)
    assert_equal %w[workflow_call workflow_dispatch], (document['on'] || document[true]).keys
    assert_equal false, document.fetch('concurrency').fetch('cancel-in-progress')
    jobs = document.fetch('jobs')
    assert_equal %w[remote_servers build sign], jobs.keys
    assert_equal 'remote_servers', jobs.fetch('build').fetch('needs')
    assert_equal true, jobs.fetch('remote_servers').fetch('with').fetch('sign_macos')
    assert_equal '1', jobs.fetch('build').fetch('env').fetch('MOEKA_REQUIRE_REMOTE_BUNDLE')
    assert_equal 'build', jobs.fetch('sign').fetch('needs')
    assert_equal 'release-signing', jobs.fetch('sign').fetch('environment')
    %w[build sign].each do |name|
      job = jobs.fetch(name)
      assert_equal true, job.fetch('if').include?("github.repository == 'MoekaProject/moeka' && github.ref == 'refs/heads/main'")
      checkout = job.fetch('steps').find { |step| step.fetch('uses', '').start_with?('actions/checkout@') }
      assert_equal '${{ inputs.source_sha || github.sha }}', checkout.fetch('with').fetch('ref')
    end
    assert_empty jobs.fetch('build').to_s.scan(/secrets\./)
    steps = jobs.fetch('sign').fetch('steps')
    assert_empty steps.to_s.scan(/rust-build-cache|sccache|rust-cache/)
    credential_steps = steps.select { |step| step.to_s.include?('secrets.') }
    assert_equal 1, credential_steps.length
    assert_equal 'bash scripts/ci-macos-signing.sh setup', credential_steps.first.fetch('run')
    cleanup = steps.find { |step| step['run'] == 'bash scripts/ci-macos-signing.sh cleanup' }
    assert_equal 'always()', cleanup.fetch('if')
    assert_empty document.to_s.scan(/make publish|gh release|git push|PUBLIC_RELEASE_TOKEN/)
    uploads = steps.select { |step| step.fetch('uses', '').start_with?('actions/upload-artifact@') }
    assert_equal ['${{ runner.temp }}/moeka-notary-results/', 'dist/'], uploads.map { |step| step.fetch('with').fetch('path') }
  end

  def test_official_release_gates_writes_and_separates_signing_from_publication
    path = File.expand_path('../.github/workflows/publish.yml', __dir__)
    document = CommunityChecks.yaml_file(path)
    assert_empty CommunityChecks.workflow(document, path)
    assert_equal ['workflow_dispatch'], (document['on'] || document[true]).keys
    jobs = document.fetch('jobs')
    assert_equal %w[preflight macos portable], jobs.fetch('publish').fetch('needs')
    assert_equal 'publish', jobs.fetch('homebrew').fetch('needs')
    assert_equal './.github/workflows/signed-macos.yml', jobs.fetch('macos').fetch('uses')
    assert_equal %w[APPLE_APP_SPECIFIC_PASSWORD APPLE_ID MACOS_CERTIFICATE_BASE64 MACOS_CERTIFICATE_PASSWORD],
                 jobs.fetch('macos').fetch('secrets').keys.sort
    assert_equal 'release-signing', jobs.fetch('preflight').fetch('environment')
    assert_equal %w[preflight remote_servers], jobs.fetch('portable').fetch('needs')
    assert_equal %w[preflight remote_servers], jobs.fetch('macos').fetch('needs')
    assert_equal 'preflight', jobs.fetch('remote_servers').fetch('needs')
    assert_equal true, jobs.fetch('remote_servers').fetch('with').fetch('sign_macos')
    assert_equal '1', jobs.fetch('portable').fetch('env').fetch('MOEKA_REQUIRE_REMOTE_BUNDLE')
    assert_empty jobs.fetch('portable').to_s.scan(/secrets\./)
    assert_empty jobs.fetch('publish').to_s.scan(/MACOS_CERTIFICATE|APPLE_APP_SPECIFIC_PASSWORD|make signed|cargo build/)
    assert_equal({ 'contents' => 'write' }, jobs.fetch('publish').fetch('permissions'))
    assert_equal({ 'contents' => 'write' }, jobs.fetch('homebrew').fetch('permissions'))
    assert_empty document.to_s.scan(/MOEKA_PUBLIC_RELEASE_TOKEN|SOURCE_REPO_TOKEN|MoekaProject\/community|homebrew-moeka/)
    unsafe = Marshal.load(Marshal.dump(document))
    unsafe['jobs']['publish']['if'] = 'true'
    refute_empty CommunityChecks.workflow(unsafe, path)
    unsafe = Marshal.load(Marshal.dump(document))
    unsafe['jobs']['portable']['permissions'] = { 'contents' => 'write' }
    refute_empty CommunityChecks.workflow(unsafe, path)
    unsafe = Marshal.load(Marshal.dump(document))
    unsafe['jobs']['homebrew']['needs'] = 'preflight'
    refute_empty CommunityChecks.workflow(unsafe, path)
    unsafe = Marshal.load(Marshal.dump(document))
    unsafe['jobs']['publish']['if'].sub!('inputs.candidate_only != true && ', '')
    refute_empty CommunityChecks.workflow(unsafe, path)
    assert_equal %w[preflight macos portable], jobs.fetch('verify-candidate').fetch('needs')
    assert_empty jobs.fetch('verify-candidate').to_s.scan(/secrets\.|contents.*write|gh release|git push/)
    %w[preflight publish homebrew verify-candidate].each do |name|
      assert_empty jobs.fetch(name).to_s.scan(/rust-build-cache|sccache|rust-cache/)
    end
  end

  def test_compiler_cache_is_read_only_outside_trusted_main
    path = File.expand_path('../.github/actions/rust-build-cache/action.yml', __dir__)
    action = CommunityChecks.yaml_file(path)
    steps = action.fetch('runs').fetch('steps')
    assert_empty CommunityChecks.action_steps(steps, path)
    refute_empty CommunityChecks.action_steps([{ 'uses' => 'mozilla-actions/sccache-action@v0.0.9' }], path)
    cases = [
      ['MoekaProject/moeka', 'refs/heads/main', 'push', 'false', true],
      ['MoekaProject/moeka', 'refs/heads/main', 'workflow_dispatch', 'true', false],
      ['MoekaProject/moeka', 'refs/heads/main', 'pull_request', 'false', false],
      ['MoekaProject/moeka', 'refs/pull/1/merge', 'pull_request', 'false', false],
      ['example/fork', 'refs/heads/main', 'push', 'false', false]
    ]
    Dir.mktmpdir('moeka-cache-policy-') do |root|
      cases.each do |repository, ref, event, read_only, writable|
        output = File.join(root, 'environment')
        File.write(output, '')
        environment = {
          'GITHUB_REPOSITORY' => repository, 'GITHUB_REF' => ref,
          'GITHUB_EVENT_NAME' => event, 'READ_ONLY' => read_only,
          'RUNNER_OS' => 'Linux', 'IMAGE' => 'ubuntu-22.04',
          'LANE' => 'gpui-tests',
          'TARGET' => 'x86_64-unknown-linux-gnu', 'GITHUB_ENV' => output
        }
        stdout, stderr, status = Open3.capture3(environment, 'bash', '-c',
          "rustc() { :; }; rustup() { :; };\n#{steps.first.fetch('run')}")
        raise "Cache policy failed: #{stdout}#{stderr}" unless status.success?
        lines = File.readlines(output, chomp: true)
        assert_equal true, lines.include?("MOEKA_CACHE_WRITE=#{writable}")
        assert_equal true, lines.include?("MOEKA_DEPENDENCY_CACHE_WRITE=#{writable}")
        assert_equal true, lines.include?("SCCACHE_GHA_RW_MODE=#{writable ? 'READ_WRITE' : 'READ_ONLY'}")
      end
      output = File.join(root, 'production-environment')
      environment = {
        'GITHUB_REPOSITORY' => 'MoekaProject/moeka', 'GITHUB_REF' => 'refs/heads/main',
        'GITHUB_EVENT_NAME' => 'push', 'READ_ONLY' => 'false', 'LANE' => 'default',
        'RUNNER_OS' => 'Linux', 'IMAGE' => 'ubuntu-22.04',
        'TARGET' => 'x86_64-unknown-linux-gnu', 'GITHUB_ENV' => output
      }
      _, stderr, status = Open3.capture3(environment, 'bash', '-c',
        "rustc() { :; }; rustup() { :; };\n#{steps.first.fetch('run')}")
      raise stderr unless status.success?
      lines = File.readlines(output, chomp: true)
      assert_equal true, lines.include?('MOEKA_CACHE_LANE=gpui-tests')
      assert_equal true, lines.include?('MOEKA_DEPENDENCY_CACHE_WRITE=false')
    end
  end

  def test_ci_selects_native_changes_and_pr_matrix_without_hiding_failures
    assert_equal false, CiChanges.native?(%w[README.md Casks/moeka.rb packaging/release-notes/v0.1.38.md])
    %w[src/main.rs Cargo.lock build.rs Makefile vendor/gpui/src/app.rs assets/icon.png docs/help-en.md .github/workflows/ci.yml scripts/ci-changes.rb].each do |path|
      assert_equal true, CiChanges.native?(['README.md', path])
    end
    assert_equal nil, CiChanges.changed_paths('0' * 40)
    path = File.expand_path('../.github/workflows/ci.yml', __dir__)
    jobs = CommunityChecks.yaml_file(path).fetch('jobs')
    selector = jobs.fetch('community').fetch('steps').find { |step| step['id'] == 'matrix' }
    Dir.mktmpdir('moeka-ci-matrix-') do |root|
      %w[true false].each do |full|
        output = File.join(root, 'matrix')
        File.write(output, '')
        stdout, stderr, status = Open3.capture3({ 'FULL' => full, 'GITHUB_OUTPUT' => output },
          'bash', '-c', selector.fetch('run'))
        raise "Matrix selection failed: #{stdout}#{stderr}" unless status.success?
        File.readlines(output).each do |line|
          entries = JSON.parse(line.split('=', 2).last)
          assert_equal(full == 'true' ? %w[x86_64 arm64] : %w[x86_64], entries.map { |entry| entry.fetch('arch') })
        end
      end
    end
    result = jobs.fetch('result')
    assert_equal 'always()', result.fetch('if')
    results_check = result.fetch('steps').first.fetch('run')
    [%w[success skipped], %w[success success], %w[failure skipped], %w[cancelled success]].each do |results|
      needs = results.each_with_index.to_h { |value, i| [i.to_s, { 'result' => value }] }
      _, _, status = Open3.capture3({ 'RESULTS' => JSON.generate(needs) }, 'bash', '-c', results_check)
      assert_equal(results.all? { |value| %w[success skipped].include?(value) }, status.success?)
    end
  end

  def test_local_links_and_heading_fragments
    Dir.mktmpdir('moeka-community-check-') do |root|
      File.write(File.join(root, 'guide.md'), "# Build and test\n\n## Build and test\n")
      readme = File.join(root, 'README.md')
      File.write(readme, "[Guide](guide.md#build-and-test)\n[Duplicate](guide.md#build-and-test-1)\n[Web](https://example.com)\n")
      assert_empty CommunityChecks.local_links(readme)
      File.write(readme, "[Missing](absent.md)\n[Wrong](guide.md#absent)\n")
      assert_equal 2, CommunityChecks.local_links(readme).size
    end
  end

  def test_yaml_uses_safe_loading_without_aliases
    Dir.mktmpdir('moeka-community-yaml-') do |root|
      path = File.join(root, 'sample.yml')
      File.write(path, "name: example\nbody: []\n")
      assert_equal 'example', CommunityChecks.yaml_file(path)['name']
      File.write(path, "one: &value test\ntwo: *value\n")
      assert_raises(Psych::Exception) { CommunityChecks.yaml_file(path) }
    end
  end

  def test_consolidated_documentation_has_no_retired_file_requirements
    Dir.mktmpdir('moeka-community-docs-') do |root|
      Dir.mkdir(File.join(root, '.github'))
      %w[README.md AGENTS.md SECURITY.md CODE_OF_CONDUCT.md LICENSE THIRD_PARTY_NOTICES.md .github/PULL_REQUEST_TEMPLATE.md].each do |path|
        File.write(File.join(root, path), "# Guide\n")
      end
      assert_empty CommunityChecks.check(root)
      File.delete(File.join(root, 'AGENTS.md'))
      assert_equal ['Missing contributor resource: AGENTS.md'], CommunityChecks.check(root)
    end
  end

  def test_submission_templates_require_english_titles_and_descriptions
    root = File.expand_path('..', __dir__)
    forms = Dir.glob(File.join(root, '.github/ISSUE_TEMPLATE/*.yml')).reject do |path|
      File.basename(path) == 'config.yml'
    end
    assert_equal ['request.yml'], forms.map { |path| File.basename(path) }
    assert_equal({ 'blank_issues_enabled' => false },
                 CommunityChecks.yaml_file(File.join(root, '.github/ISSUE_TEMPLATE/config.yml')))
    forms.each do |path|
      document = CommunityChecks.yaml_file(path)
      assert_equal true, document.fetch('description').include?('English only.')
      body = document.fetch('body')
      assert_equal 2, body.length
      field = body.first
      assert_equal 'textarea', field['type']
      assert_equal true, field.dig('validations', 'required')
      assert_equal true, field.dig('attributes', 'description').include?('Use English only for the issue title and description.')
      content = field.dig('attributes', 'value')
      assert_equal ['Problem', 'Expected outcome', 'Environment'], content.scan(/^### (.+)$/).flatten
      assert_equal true, content.include?('- Operating system and version:')
      assert_equal true, content.include?('- Moeka version:')
      confirmations = body.last
      assert_equal 'checkboxes', confirmations['type']
      options = confirmations.dig('attributes', 'options')
      assert_equal [
        'My issue title and description are written in English.',
        'I searched existing issues for duplicate reports or requests.',
        'I removed credentials, private documents, personal paths, and other sensitive information.'
      ], options.map { |option| option['label'] }
      assert_equal true, options.all? { |option| option['required'] == true }
    end
    template = File.read(File.join(root, '.github/PULL_REQUEST_TEMPLATE.md'))
    assert_equal true, template.include?('Please write the pull-request title and description in English.')
    assert_equal true, template.include?('- [ ] My pull-request title and description are in English.')
  end
end

tests = CommunityChecksTest.instance_methods(false).grep(/^test_/).sort
raise 'No contributor-metadata tests were discovered' if tests.empty?
tests.each { |test| CommunityChecksTest.new.public_send(test) }
puts "#{tests.size} contributor-metadata regression tests passed."
