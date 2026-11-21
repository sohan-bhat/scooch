require "yaml"
require "json"
require "digest"

ROOT = File.expand_path(ARGV.fetch(0, "."))
PRINT_LOCK = ARGV[1] == "--lock"
GITHUB = File.join(ROOT, ".github")
LOCK = File.join(GITHUB, "app", "guard.lock")

PLACES = [
  ["app.yml", "bundle-macos", "Bundle and sign"],
  ["app.yml", "bundle-windows", "Bundle and sign"],
  ["app.yml", "updater-sign", "Sign the update files"],
  ["release.yml", "app", nil],
  ["release.yml", "publish", "Release"]
].freeze
WRITES = [["release.yml", "publish"], ["pages.yml", "deploy"]].freeze
ACTIONS = %w[actions/checkout actions/download-artifact actions/upload-artifact actions/deploy-pages].freeze
SCRIPTS = %w[bundle-macos.sh bundle-windows.sh bundle-linux.sh updater-sign.sh release-sums.sh release-publish.sh].freeze
INPUTS = %w[
  crates/scooch-app/windows/sign.ps1
  crates/scooch-app/windows/hooks.nsh
  crates/scooch-app/windows/hooks.ps1
  crates/scooch-app/macos/Entitlements.plist
  crates/scooch-app/macos/Helper.plist
  crates/scooch-app/ui/package-lock.json
].freeze
PINNED_FILES = %w[workflows/app.yml workflows/release.yml].freeze
SIGN_COMMAND = "pwsh -NoProfile -File windows/sign.ps1 %1".freeze
HOOKS = "windows/hooks.nsh".freeze
ENTITLEMENTS = "macos/Entitlements.plist".freeze
FIXED_ENV = { "GH_TOKEN" => "${{ github.token }}", "TAG" => "${{ github.ref_name }}" }.freeze
ARGUMENT = /\A(?:"\$RUNNER_TEMP\/[a-z]+"|MATRIX_TARGET)\z/
HOSTED = /\A(?:ubuntu|macos|windows)-[a-z0-9.]+\z/
PLAIN = /\A[A-Za-z0-9_.+-]*\z/
JOB_KEYS = %w[name runs-on needs steps strategy timeout-minutes permissions environment if].freeze
CALL_KEYS = %w[name needs uses secrets if].freeze
STEP_KEYS = %w[name id if uses with run env shell].freeze
WITH_KEYS = {
  "actions/checkout" => %w[persist-credentials],
  "actions/download-artifact" => %w[name pattern path merge-multiple],
  "actions/upload-artifact" => %w[name path if-no-files-found],
  "actions/deploy-pages" => []
}.freeze
BANNED_TRIGGERS = %w[pull_request_target workflow_run].freeze

problems = []

def walk(node, key = nil, &block)
  case node
  when Hash
    node.each do |child, value|
      yield child.to_s, nil
      walk(value, child.to_s, &block)
    end
  when Array
    node.each { |value| walk(value, key, &block) }
  else
    yield node.to_s, key unless node.nil?
  end
end

def token_in?(expression)
  return true if expression.match?(/github\s*\.\s*token\b/i)
  expression.scan(/(?<![\w.\/'"-])github\b(.{0,3})/im).any? { |after,| !after.match?(/\A\s*\.\s*[A-Za-z_]/) }
end

def names_secret?(text, key)
  return true if text.match?(/secrets/i) || text.match?(/github_token/i)
  expressions = text.scan(/\$\{\{(.*?)\}\}/m).flatten
  expressions << text if key == "if"
  expressions.any? { |expression| token_in?(expression) }
end

def entry_mentions?(key, value)
  return true if names_secret?(key.to_s, nil)
  value.is_a?(Hash) || value.is_a?(Array) ? mentions?(value) : names_secret?(value.to_s, key.to_s)
end

def mentions?(node)
  found = false
  walk(node) { |text, key| found ||= names_secret?(text, key) }
  found
end

def writes?(permissions)
  return true if permissions.is_a?(String) && permissions.match?(/write/i)
  permissions.is_a?(Hash) && permissions.values.any? { |value| value.to_s == "write" }
end

def step_name(step)
  (step["name"] || step["uses"] || step["run"].to_s.lines.first.to_s.strip).to_s
end

def canonical(node)
  case node
  when Hash then node.map { |key, value| [key.to_s, canonical(value)] }.sort_by(&:first)
  when Array then node.map { |value| canonical(value) }
  else node
  end
end

def triggers(doc)
  on = doc.key?("on") ? doc["on"] : doc[true]
  case on
  when Hash then on.keys.map(&:to_s)
  when Array then on.map(&:to_s)
  when nil then []
  else [on.to_s]
  end
end

def read_lock
  lines = File.exist?(LOCK) ? File.readlines(LOCK, chomp: true) : []
  lines.map(&:strip).reject { |line| line.empty? }.map(&:split)
end

files = Dir.glob(File.join(GITHUB, "{workflows,actions}", "**", "*.{yml,yaml}")).sort
problems << "no workflow files under #{GITHUB}" if files.empty?

held = {}
uses_seen = []
docs = {}

files.each do |path|
  file = File.basename(path)
  where = path.sub("#{GITHUB}/", "")
  doc = YAML.safe_load(File.read(path), aliases: true) || {}
  docs[where] = doc
  workflow = where.start_with?("workflows/")
  pinned = PINNED_FILES.include?(where) || where.start_with?("actions/")

  doc.each do |key, value|
    next if key.to_s == "jobs" || (key.to_s == "runs" && value.is_a?(Hash))
    problems << "#{where} names secrets or the token at its top under #{key}" if entry_mentions?(key, value)
  end

  if workflow
    BANNED_TRIGGERS.each { |name| problems << "#{where} starts on #{name}" if triggers(doc).include?(name) }
    top = doc["permissions"]
    if !top.is_a?(Hash)
      problems << "#{where} has no top level permissions of contents read or less"
    elsif top.any? { |scope, level| scope.to_s != "contents" || !%w[read none].include?(level.to_s) }
      problems << "#{where} grants more than contents read at its top"
    end
  end

  steps_in = lambda do |job_id, steps|
    Array(steps).each do |step|
      next unless step.is_a?(Hash)
      name = step_name(step)
      problems << "#{where}, job #{job_id || '-'}, step \"#{name}\" names secrets or the token" if mentions?(step) && !PLACES.include?([file, job_id, name])
      action = step["uses"].to_s.split("@", 2).first
      if action == "actions/checkout" && step.dig("with", "persist-credentials").to_s != "false"
        problems << "#{where}, job #{job_id || '-'}, a checkout keeps its credentials, set persist-credentials false"
      end
      uses_seen << [where, step["uses"].to_s] if pinned && step["uses"]
    end
  end

  if doc["runs"].is_a?(Hash)
    rest = doc["runs"].reject { |key, _| key.to_s == "steps" }
    problems << "#{where} names secrets in runs" if mentions?(rest)
    steps_in.call(nil, doc["runs"]["steps"])
  end

  (doc["jobs"] || {}).each do |job_id, job|
    next unless job.is_a?(Hash)
    job.each do |key, value|
      next if key.to_s == "steps"
      next unless entry_mentions?(key, value)
      next if PLACES.include?([file, job_id, nil]) && key.to_s == "secrets" && value.to_s == "inherit"
      problems << "#{where}, job #{job_id} names secrets or the token under #{key}"
    end
    if writes?(job["permissions"]) && !WRITES.include?([file, job_id])
      problems << "#{where}, job #{job_id} has a write permission and is not on the guard's list"
    end
    steps_in.call(job_id, job["steps"])
    uses_seen << [where, job["uses"].to_s] if pinned && job["uses"]
    if PLACES.any? { |place_file, place_job, _| place_file == file && place_job == job_id } || WRITES.include?([file, job_id])
      held[[file, job_id]] = { where: where, job: job, doc: doc }
    end
  end
end

def plain_run?(step)
  run = step["run"].to_s.strip
  return false if run.include?("\n")
  words = run.gsub("${{ matrix.target }}", "MATRIX_TARGET").split(/\s+/)
  return false unless words[0] == "bash" && File.dirname(words[1].to_s) == ".github/app" && SCRIPTS.include?(File.basename(words[1].to_s))
  words.drop(2).all? { |word| word.match?(ARGUMENT) }
end

scripts = []
held.each do |(file, job_id), info|
  where, job, doc = info.values_at(:where, :job, :doc)
  label = "#{where}, job #{job_id}"
  %w[env defaults].each { |key| problems << "#{where} has #{key} at its top and holds a secret or a write token in job #{job_id}" if doc.key?(key) }
  if job.key?("uses")
    (job.keys.map(&:to_s) - CALL_KEYS).each { |key| problems << "#{label} calls a workflow and has #{key}" }
  else
    (job.keys.map(&:to_s) - JOB_KEYS).each { |key| problems << "#{label} holds a secret or a write token and has #{key}" }
    problems << "#{label} runs on #{job['runs-on'].inspect}, not a GitHub hosted label" unless job["runs-on"].is_a?(String) && job["runs-on"].match?(HOSTED)
  end
  problems << "#{label} holds a secret or a write token but does not wait for the guard" unless Array(job["needs"]).include?("guard")
  walk(job.dig("strategy", "matrix")) do |text, _|
    problems << "#{label} has a matrix value that is not a plain word, #{text.inspect}" unless text.match?(PLAIN)
  end
  Array(job["steps"]).each do |step|
    name = step_name(step)
    at = "#{label}, step \"#{name}\""
    (step.keys.map(&:to_s) - STEP_KEYS).each { |key| problems << "#{at} has #{key}" }
    problems << "#{at} has shell #{step['shell'].inspect}" if step.key?("shell") && step["shell"] != "bash"
    (step["env"] || {}).each do |key, value|
      fixed = FIXED_ENV[key.to_s]
      ok = fixed ? value.to_s == fixed : value.to_s == "${{ secrets.#{key} }}"
      problems << "#{at} sets env #{key} to #{value.inspect}" unless ok
    end
    if step.key?("uses")
      uses_seen << [where, step["uses"].to_s]
      action, pin = step["uses"].to_s.split("@", 2)
      problems << "#{at} uses #{step['uses']}, not a listed action at a commit" unless ACTIONS.include?(action) && pin.to_s.match?(/\A[0-9a-f]{40}\z/)
      problems << "#{at} has both uses and run" if step.key?("run")
      with = step["with"] || {}
      (with.keys.map(&:to_s) - WITH_KEYS.fetch(action, [])).each { |key| problems << "#{at} gives #{action} the input #{key}" }
      if action == "actions/download-artifact" && !with["path"].to_s.start_with?("${{ runner.temp }}/")
        problems << "#{at} downloads into the checkout at #{with['path'].inspect}, use a folder under runner.temp"
      end
    elsif step.key?("run")
      if plain_run?(step)
        scripts << step["run"].split(/\s+/)[1]
      else
        problems << "#{at} is not one call of a listed script with plain arguments"
      end
    else
      problems << "#{at} neither runs a listed script nor uses a listed action"
    end
  end
end

frozen = (scripts.uniq + INPUTS).uniq
queue = frozen.select { |path| path.start_with?(".github/app/") }
until queue.empty?
  path = queue.shift
  full = File.join(ROOT, path)
  next unless File.file?(full)
  File.read(full).scan(%r{\.github/app/([A-Za-z0-9_.-]+\.(?:sh|rb|py|ps1))}).flatten.each do |name|
    found = ".github/app/#{name}"
    next if frozen.include?(found)
    frozen << found
    queue << found
  end
end

current = []
held.keys.sort.each do |file, job_id|
  info = held[[file, job_id]]
  doc = info[:doc]
  payload = { "file" => file, "job" => job_id, "env" => doc["env"], "defaults" => doc["defaults"], "permissions" => doc["permissions"], "definition" => info[:job] }
  current << ["job", "#{file}:#{job_id}", Digest::SHA256.hexdigest(JSON.generate(canonical(payload)))]
end
frozen.sort.each do |path|
  full = File.join(ROOT, path)
  current << ["file", path, File.file?(full) ? Digest::SHA256.file(full).hexdigest : "missing"]
end

lock = read_lock
actions = lock.select { |line| line[0] == "action" }

if PRINT_LOCK
  current.each { |line| puts line.join(" ") }
  actions.each { |line| puts line.join(" ") }
  exit 0
end

locked = lock.reject { |line| line[0] == "action" }.to_h { |kind, name, value| [[kind, name], value] }
current.each do |kind, name, value|
  if locked[[kind, name]] != value
    problems << "guard.lock: #{kind} #{name} is now #{value}, the lock says #{locked[[kind, name]] || 'nothing'}"
  end
end
(locked.keys - current.map { |kind, name, _| [kind, name] }).each do |kind, name|
  problems << "guard.lock names #{kind} #{name}, which no job that holds a secret or a write token uses"
end

pins = actions.to_h { |_, repo, sha, _| [[repo, sha], true] }
uses_seen.uniq.each do |where, uses|
  next if uses.start_with?("./")
  repo, sha = uses.split("@", 2)
  repo = repo.split("/").first(2).join("/")
  if !sha.to_s.match?(/\A[0-9a-f]{40}\z/)
    problems << "#{where} uses #{uses}, not pinned by a commit"
  elsif !pins[[repo, sha]]
    problems << "#{where} uses #{uses}, a commit the lock does not tie to a release tag"
  end
end

configs = Dir.glob(File.join(ROOT, "crates", "**", "{tauri,Tauri}*")).reject { |path| path.match?(%r{/(target|node_modules|gen)/}) }
configs.each do |path|
  name = path.sub("#{ROOT}/", "")
  if path.match?(/\.(json5|toml)\z/i)
    problems << "#{name} is a Tauri config the guard does not read, keep the config in JSON"
    next
  end
  next unless path.end_with?(".conf.json")
  config = JSON.parse(File.read(path))
  stack = [[config, []]]
  until stack.empty?
    node, trail = stack.pop
    case node
    when Hash
      node.each do |key, value|
        at = (trail + [key]).join(".")
        problems << "#{name} sets #{at}, a command the bundler runs" if key == "beforeBundleCommand"
        problems << "#{name} sets #{at}, a program the CLI runs in place of cargo" if key == "runner" && trail == ["build"]
        problems << "#{name} sets #{at} to #{value.inspect}, not #{SIGN_COMMAND.inspect}" if key == "signCommand" && value != SIGN_COMMAND
        problems << "#{name} sets #{at}, an NSIS script makensis runs" if key == "template" && trail.last == "nsis"
        problems << "#{name} sets #{at} to #{value.inspect}, not #{HOOKS.inspect}" if key == "installerHooks" && value != HOOKS
        problems << "#{name} sets #{at} to #{value.inspect}, not #{ENTITLEMENTS.inspect}" if key == "entitlements" && trail.last == "macOS" && value != ENTITLEMENTS
        stack << [value, trail + [key]]
      end
    when Array
      node.each { |value| stack << [value, trail] }
    end
  end
end

if problems.empty?
  puts "The secrets and the token appear only in #{PLACES.map { |place| place.compact.join(' ') }.join(', ')}. Write permissions only in #{WRITES.map { |place| place.join(' ') }.join(', ')}. Every step of those jobs is a pinned action or a listed script, and #{current.size} jobs and files match guard.lock."
else
  puts problems
  exit 1
end
