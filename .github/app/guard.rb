require "yaml"

PLACES = [
  ["app.yml", "bundle-macos", "Bundle and sign"],
  ["app.yml", "bundle-windows", "Bundle and sign"],
  ["app.yml", "updater-sign", "Sign the update files"],
  ["release.yml", "app", nil]
].freeze

ACTIONS = %w[actions/checkout actions/download-artifact actions/upload-artifact].freeze
SCRIPTS = %w[bundle-macos.sh bundle-windows.sh bundle-linux.sh updater-sign.sh].freeze
ARGUMENT = /\A(?:"\$RUNNER_TEMP\/[a-z]+"|MATRIX_TARGET)\z/
WORD = /secrets/i

root = ARGV.fetch(0, ".github")
problems = []

def strings(node, &block)
  case node
  when Hash
    node.each do |key, value|
      yield key.to_s
      strings(value, &block)
    end
  when Array
    node.each { |value| strings(value, &block) }
  else
    yield node.to_s unless node.nil?
  end
end

def mentions?(node)
  found = false
  strings(node) { |text| found ||= text.match?(WORD) }
  found
end

def step_name(step)
  (step["name"] || step["uses"] || step["run"].to_s.lines.first.to_s.strip).to_s
end

def allowed?(file, job, step)
  PLACES.include?([file, job, step])
end

def plain_step?(step)
  keys = step.keys.map(&:to_s) - %w[name env shell]
  if keys == ["uses"] || keys.sort == %w[uses with]
    action, pin = step["uses"].to_s.split("@", 2)
    return ACTIONS.include?(action) && pin.to_s.match?(/\A[0-9a-f]{40}\z/)
  end
  return false unless keys == ["run"]
  words = step["run"].to_s.strip.gsub("${{ matrix.target }}", "MATRIX_TARGET").split(/\s+/)
  return false if step["run"].to_s.strip.include?("\n")
  return false unless words[0] == "bash" && words[1].to_s.start_with?(".github/app/")
  return false unless SCRIPTS.include?(File.basename(words[1])) && File.dirname(words[1]) == ".github/app"
  words.drop(2).all? { |word| word.match?(ARGUMENT) }
end

files = Dir.glob(File.join(root, "{workflows,actions}", "**", "*.{yml,yaml}")).sort
problems << "no workflow files under #{root}" if files.empty?

files.each do |path|
  file = File.basename(path)
  where = path.sub("#{root}/", "")
  doc = YAML.safe_load(File.read(path), aliases: true) || {}
  doc.each do |key, value|
    next if key.to_s == "jobs" || (key.to_s == "runs" && value.is_a?(Hash))
    problems << "#{where} names secrets at its top under #{key}" if key.to_s.match?(WORD) || mentions?(value)
  end
  steps_of = lambda do |job_id, steps|
    Array(steps).each do |step|
      next unless step.is_a?(Hash)
      name = step_name(step)
      problems << "#{where}, job #{job_id || '-'}, step \"#{name}\" names secrets" if mentions?(step) && !allowed?(file, job_id, name)
    end
  end
  if doc["runs"].is_a?(Hash)
    rest = doc["runs"].reject { |key, _| key.to_s == "steps" }
    problems << "#{where} names secrets in runs" if mentions?(rest)
    steps_of.call(nil, doc["runs"]["steps"])
  end
  (doc["jobs"] || {}).each do |job_id, job|
    next unless job.is_a?(Hash)
    job.each do |key, value|
      next if key.to_s == "steps"
      next unless key.to_s.match?(WORD) || mentions?(value)
      problems << "#{where}, job #{job_id} names secrets under #{key}" unless allowed?(file, job_id, nil) && key.to_s == "secrets" && value.to_s == "inherit"
    end
    steps_of.call(job_id, job["steps"])
    held = PLACES.any? { |place_file, place_job, place_step| place_file == file && place_job == job_id && place_step }
    next unless held
    problems << "#{where}, job #{job_id} holds a secret but does not wait for the guard" unless Array(job["needs"]).include?("guard")
    %w[container services].each { |key| problems << "#{where}, job #{job_id} holds a secret and has #{key}" if job.key?(key) }
    Array(job["steps"]).each do |step|
      problems << "#{where}, job #{job_id}, step \"#{step_name(step)}\" is not a pinned listed action or one call of a listed script" unless plain_step?(step)
    end
  end
end

if problems.empty?
  puts "The word secrets appears only in #{PLACES.map { |place| place.compact.join(' ') }.join(', ')}, and every step of a job that holds one is a pinned action or a listed script."
else
  puts problems
  exit 1
end
