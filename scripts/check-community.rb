#!/usr/bin/env ruby
# Offline checks for contributor metadata; no GitHub credentials or network.
require 'pathname'
require 'yaml'
require 'uri'

module CommunityChecks
  module_function

  def yaml_file(path)
    YAML.safe_load(File.read(path), permitted_classes: [], permitted_symbols: [], aliases: false)
  end

  def issue_form(document, name)
    errors = []
    unless document.is_a?(Hash) && %w[name description body].all? { |key| document.key?(key) }
      return ["#{name}: expected an issue form with name, description, and body"]
    end
    %w[name description].each do |key|
      errors << "#{name}: missing #{key}" unless document[key].is_a?(String) && !document[key].strip.empty?
    end
    return errors + ["#{name}: body must be an array"] unless document['body'].is_a?(Array)

    ids = []
    document['body'].each do |field|
      unless field.is_a?(Hash) && %w[markdown input textarea dropdown checkboxes].include?(field['type'])
        errors << "#{name}: invalid issue field type"
        next
      end
      attributes = field['attributes']
      unless attributes.is_a?(Hash)
        errors << "#{name}: field attributes must be a mapping"
        next
      end
      if field['type'] == 'markdown'
        errors << "#{name}: markdown field needs content" unless attributes['value'].is_a?(String)
        next
      end
      id = field['id']
      errors << "#{name}: field needs a unique stable id" unless id.is_a?(String) && id.match?(/\A[a-z0-9_-]+\z/) && !ids.include?(id)
      ids << id
      errors << "#{name}: #{id} needs a label" unless attributes['label'].is_a?(String) && !attributes['label'].strip.empty?
      if field.key?('validations')
        validations = field['validations']
        unless validations.is_a?(Hash) && [true, false].include?(validations['required'])
          errors << "#{name}: #{id} required validation must be boolean"
        end
      end
      next unless %w[dropdown checkboxes].include?(field['type'])

      options = attributes['options']
      valid = options.is_a?(Array) && !options.empty? && options.all? do |option|
        if field['type'] == 'dropdown'
          option.is_a?(String) && !option.strip.empty?
        else
          option.is_a?(Hash) && option['label'].is_a?(String) &&
            (!option.key?('required') || [true, false].include?(option['required']))
        end
      end
      errors << "#{name}: #{id} has invalid choices" unless valid
    end
    errors
  end

  def workflow(document, name)
    return ["#{name}: workflow must be a mapping"] unless document.is_a?(Hash)
    errors = []
    # Psych uses YAML 1.1, in which an unquoted `on` key becomes true.
    events = document['on'] || document[true]
    events = events.keys if events.is_a?(Hash)
    if (Array(events) & %w[pull_request_target workflow_run]).any?
      errors << "#{name}: privileged workflow triggers require a separate reviewed design"
    end
    errors << "#{name}: expected read-only contents permission" unless document['permissions'] == { 'contents' => 'read' }
    jobs = document['jobs']
    return errors + ["#{name}: missing jobs"] unless jobs.is_a?(Hash) && !jobs.empty?

    jobs.each do |job_name, job|
      if job.fetch('permissions', {}).values.include?('write')
        errors << "#{name}/#{job_name}: contributor workflows must not request write permissions"
      end
      job.fetch('steps', []).each do |step|
        action = step['uses']
        next unless action
        unless action.start_with?('./') || action.match?(/\A[^@\s]+@[0-9a-f]{40}\z/)
          errors << "#{name}/#{job_name}: action must use an immutable commit SHA: #{action}"
        end
        if action.start_with?('actions/checkout@') && step.fetch('with', {})['persist-credentials'] != false
          errors << "#{name}/#{job_name}: checkout must not persist credentials"
        end
      end
    end
    errors
  end

  def headings(source)
    counts = Hash.new(0)
    source.scan(/^ {0,3}\#{1,6}\s+(.+?)\s*#*\s*$/).map do |match|
      slug = match.first.downcase.gsub(/[^\p{L}\p{N}_ -]/, '').tr(' ', '-')
      number = counts[slug]
      counts[slug] += 1
      number.zero? ? slug : "#{slug}-#{number}"
    end
  end

  def local_links(path)
    source = File.read(path).gsub(/^```[^\n]*\n.*?^```[^\n]*$/m, '')
    targets = source.scan(/\[[^\]\n]*\]\(([^)\n]+)\)/).flatten
    targets.concat(source.scan(/^\[[^\]\n]+\]:\s+(\S+)/).flatten)
    targets.each_with_object([]) do |target, errors|
      target = target.strip.sub(/\s+["'].*\z/, '').sub(/\A</, '').sub(/>\z/, '')
      next if target.match?(/\A[a-z][a-z0-9+.-]*:/i) || target.start_with?('//')
      relative, fragment = target.split('#', 2)
      destination = relative.empty? ? path : File.expand_path(URI::DEFAULT_PARSER.unescape(relative), File.dirname(path))
      unless File.exist?(destination)
        errors << "#{path}: missing local link #{target}"
        next
      end
      next unless fragment && File.file?(destination) && File.extname(destination) == '.md'

      fragment = URI::DEFAULT_PARSER.unescape(fragment)
      content = File.read(destination)
      anchors = headings(content) + content.scan(/<(?:a|h[1-6])\b[^>]*(?:id|name)=["']([^"']+)["']/i).flatten
      errors << "#{path}: missing anchor #{target}" unless anchors.include?(fragment)
    end
  end

  def check(root)
    errors = []
    %w[README.md AGENTS.md SECURITY.md CODE_OF_CONDUCT.md LICENSE THIRD_PARTY_NOTICES.md .github/PULL_REQUEST_TEMPLATE.md].each do |path|
      errors << "Missing contributor resource: #{path}" unless File.file?(File.join(root, path))
    end
    Dir.glob(File.join(root, '.github/ISSUE_TEMPLATE/*.yml')).sort.each do |path|
      document = yaml_file(path)
      if File.basename(path) == 'config.yml'
        errors << "#{path}: blank issues must be disabled" unless document['blank_issues_enabled'] == false
        links = document['contact_links']
        if document.key?('contact_links') && (!links.is_a?(Array) || links.empty?)
          errors << "#{path}: contact links must be a nonempty array when provided"
        end
      else
        errors.concat(issue_form(document, path))
      end
    end
    Dir.glob(File.join(root, '.github/workflows/*.yml')).sort.each do |path|
      errors.concat(workflow(yaml_file(path), path))
    end
    documents = Dir.glob(File.join(root, '*.md')) +
      Dir.glob(File.join(root, 'packaging/**/*.md')) +
      Dir.glob(File.join(root, '.github/**/*.md'))
    documents.sort.each { |path| errors.concat(local_links(path)) }
    errors
  end
end

if $PROGRAM_NAME == __FILE__
  begin
    errors = CommunityChecks.check(File.expand_path('..', __dir__))
    errors.each { |error| warn error }
    puts 'Community metadata, workflow policy, and local documentation links passed.' if errors.empty?
    exit(errors.empty? ? 0 : 1)
  rescue Psych::Exception, SystemCallError => error
    warn "Community checks could not complete: #{error.message}"
    exit 1
  end
end
