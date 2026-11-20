require "yaml"

NAMES = %w[
  APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_ISSUER APPLE_API_KEY
  APPLE_API_PRIVATE_KEY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID WINDOWS_CERTIFICATE WINDOWS_CERTIFICATE_PASSWORD
  TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD TAURI_UPDATER_PUBKEY
].freeze

ALLOWED = {
  "Sign for macOS" => %w[
    APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_ISSUER APPLE_API_KEY
    APPLE_API_PRIVATE_KEY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID
  ],
  "Sign for Windows" => %w[WINDOWS_CERTIFICATE WINDOWS_CERTIFICATE_PASSWORD],
  "Updater signatures" => %w[TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD],
  "Updater switch" => %w[TAURI_UPDATER_PUBKEY]
}.freeze

SIGNING_JOBS = %w[build-macos build-windows build-linux].freeze

root = ARGV.fetch(0, ".github")
problems = []

def named(env)
  (env || {}).keys.map(&:to_s) & NAMES
end

def secret_refs(node)
  node.to_s.scan(/secrets\.([A-Za-z0-9_]+)/).flatten.uniq
end

def check_step(problems, where, job, step)
  name = step["name"].to_s
  set = named(step["env"])
  refs = secret_refs(step.reject { |key, _| key == "env" })
  env_refs = secret_refs(step["env"])
  allowed = ALLOWED.fetch(name, [])
  set.each do |key|
    problems << "#{where}, step \"#{name}\" sets #{key}" unless allowed.include?(key)
  end
  env_refs.each do |key|
    problems << "#{where}, step \"#{name}\" reads secret #{key}" unless allowed.include?(key)
  end
  refs.each do |key|
    problems << "#{where}, step \"#{name}\" reads secret #{key} outside its env"
  end
  if (set.any? || env_refs.any?) && job && !SIGNING_JOBS.include?(job)
    problems << "#{where}, step \"#{name}\" holds a signing name in job #{job}, which signs nothing"
  end
end

Dir.glob(File.join(root, "workflows", "*.{yml,yaml}")).sort.each do |file|
  flow = YAML.safe_load(File.read(file), aliases: true) || {}
  where = File.basename(file)
  named(flow["env"]).each { |key| problems << "#{where} sets #{key} for every job" }
  secret_refs(flow["env"]).each { |key| problems << "#{where} reads secret #{key} for every job" }
  (flow["jobs"] || {}).each do |job, body|
    named(body["env"]).each { |key| problems << "#{where}, job #{job} sets #{key} for every step" }
    secret_refs(body["env"]).each { |key| problems << "#{where}, job #{job} reads secret #{key} for every step" }
    secret_refs(body["container"]).each { |key| problems << "#{where}, job #{job} hands secret #{key} to its container" }
    secret_refs(body["with"]).each { |key| problems << "#{where}, job #{job} hands secret #{key} to a called workflow" }
    (body["steps"] || []).each { |step| check_step(problems, "#{where}, job #{job}", job, step) }
  end
end

Dir.glob(File.join(root, "actions", "*", "action.{yml,yaml}")).sort.each do |file|
  action = YAML.safe_load(File.read(file), aliases: true) || {}
  where = file.sub("#{root}/", "")
  (action.dig("runs", "steps") || []).each { |step| check_step(problems, where, nil, step) }
end

if problems.empty?
  puts "No signing name reaches a step outside #{ALLOWED.keys.join(', ')}."
else
  puts problems
  exit 1
end
