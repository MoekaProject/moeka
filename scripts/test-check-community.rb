#!/usr/bin/env ruby
require 'tmpdir'
require_relative 'check-community'

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
