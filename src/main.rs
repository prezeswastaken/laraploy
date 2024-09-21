use std::process::{Child, Command, Stdio};

const LARAVEL_PROJECT_PATH: &str = "/var/www/laravel";

const CADDY_FILE_PATH: &str = "/etc/caddy/Caddyfile";
const CADDY_FILE_TEMPLATE: &str = include_str!("../templates/Caddyfile");

fn check_if_repo_exists(url: &str) -> anyhow::Result<bool> {
    let response = reqwest::blocking::get(url)?;
    Ok(response.status().is_success())
}

fn get_env_keys_from_env_example() -> anyhow::Result<Vec<String>> {
    let env_example_path = format!("{}/.env.example", LARAVEL_PROJECT_PATH);
    let env_example_file = std::fs::read_to_string(env_example_path)?;
    let env_keys = env_example_file
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split('=').next())
        .map(|key| key.to_string())
        .collect();
    Ok(env_keys)
}

fn prompt_for_env_values(env_keys: Vec<String>) -> anyhow::Result<Vec<(String, String)>> {
    let mut env_values = Vec::new();
    for key in env_keys {
        let mut value =
            cliclack::input(format!("Enter the value for {}: ", key).as_str()).required(false).interact::<String>()?;
        if !value.is_empty() {
            if key == "APP_NAME" && !value.starts_with('"') && !value.ends_with('"') {
                value = format!("\"{}\"", value);
            }
            env_values.push((key, value));
        }
    }
    Ok(env_values)
}

fn make_env_file_contents(env_values: Vec<(String, String)>) -> anyhow::Result<String> {
    let env_file_contents = env_values
        .iter()
        .fold(String::new(), |acc, (key, value)| {
            format!("{}\n{}={}", acc, key, value)
        })
        .trim()
        .to_string();
    Ok(env_file_contents)
}

fn create_env_file(env_file_contents: String) -> anyhow::Result<()> {
    let env_file_path = format!("{LARAVEL_PROJECT_PATH}/.env");
    std::fs::write(env_file_path, env_file_contents)?;
    Ok(())
}

fn replace_placeholders_in_caddyfile(domain: String, reverb_port: String, reverb_server_port: String) -> String {
    let caddyfile = CADDY_FILE_TEMPLATE
        .replace("{{DOMAIN}}", &domain)
        .replace("{{REVERB_PORT}}", &reverb_port)
        .replace("{{REVERB_SERVER_PORT}}", &reverb_server_port)
        .replace("{{LARAVEL_PROJECT_PATH}}", LARAVEL_PROJECT_PATH);

    caddyfile
}

fn create_caddyfile(domain: String, reverb_port: String, reverb_server_port: String) -> anyhow::Result<()> {
    let caddyfile = replace_placeholders_in_caddyfile(domain, reverb_port, reverb_server_port);
    std::fs::write(CADDY_FILE_PATH, caddyfile).map_err(|_| {anyhow::anyhow!("Error creating caddyfile!")})?;
    Ok(())
}

fn install_dependencies() -> anyhow::Result<Child> {
    let command = "apt update -y && apt install -y git php8.3 php8.3-cli php8.3-fpm php8.3-sqlite3 php8.3-mbstring php8.3-xml php8.3-curl php8.3-zip php8.3-gd php8.3-bcmath php8.3-intl php8.3-soap php8.3-opcache caddy composer";

    let child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| anyhow::anyhow!("Error while installing PHP extensions!"))?;

    Ok(child)

}

fn configure_laravel_things() -> anyhow::Result<Child> {
    let command = format!("cd {LARAVEL_PROJECT_PATH} && composer --no-interaction install && php artisan key:generate --force && touch database/database.sqlite && php artisan migrate --force && php artisan db:seed --force && php artisan storage:link && chown -R www-data:www-data {LARAVEL_PROJECT_PATH} && chmod -R 775 {LARAVEL_PROJECT_PATH}/storage && chmod -R 775 {LARAVEL_PROJECT_PATH}/bootstrap/cache");
    let child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|_| anyhow::anyhow!("Error while configuring Laravel!"))?;

    Ok(child)
}

fn clone_project(repo_url: String) -> anyhow::Result<Child> {
    let clone_path = LARAVEL_PROJECT_PATH.split('/')
        .take(LARAVEL_PROJECT_PATH.split('/').count() - 1)
        .collect::<Vec<&str>>()
        .join("/");
    println!("Clone path: {clone_path}");

    // let command = format!("cd {clone_path} && mkdir laravel && git clone {repo_url} laravel");
    let command = format!("mkdir -p {LARAVEL_PROJECT_PATH} && git clone {repo_url} {LARAVEL_PROJECT_PATH}");
    let child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| anyhow::anyhow!("Error while cloning the repository!"))?;

    Ok(child)
}

fn main() -> anyhow::Result<()> {
    cliclack::intro("Welcome to Laraploy!");
    let repo_url = cliclack::input("Enter the URL of the repository:").required(true).interact::<String>()?;
    let repo_exists = check_if_repo_exists(&repo_url)
        .map_err(|_| anyhow::anyhow!("Error while checking if repository exists!"))?;

    if !repo_exists {
        anyhow::bail!("The repository \"{repo_url}\" does not exist");
    }

    let mut handle = clone_project(repo_url)?;
    cliclack::log::info("Cloning the repository...")?;

    handle.wait()?;

    let mut handle = install_dependencies()?;

    let env_keys = get_env_keys_from_env_example()?;
    let env_values = prompt_for_env_values(env_keys)?;
    let env_file_contents = make_env_file_contents(env_values.clone())?;

    let domain = cliclack::input("Enter the domain: (ex. mysite.com, without 'www' prefix").required(true).interact::<String>()?;

    let reverb_port = env_values
        .iter()
        .find(|(key, _)| key == "REVERB_PORT")
        .map(|(_, value)| value)
        .unwrap_or(&"8001".to_string()).to_string();

    let reverb_server_port = env_values
        .iter()
        .find(|(key, _)| key == "REVERB_SERVER_PORT")
        .map(|(_, value)| value)
        .unwrap_or(&"8002".to_string()).to_string();

    cliclack::log::info("Waiting for all dependencies to be installed...")?;
    handle.wait()?;
    cliclack::log::info("Creating Caddyfile...")?;
    create_caddyfile(domain, reverb_port, reverb_server_port)?;

    cliclack::log::info("Creating .env file...")?;
    create_env_file(env_file_contents)?;

    let mut handle = configure_laravel_things()?;
    cliclack::log::info("Configuring Laravel project...")?;
    handle.wait()?;

    cliclack::outro("All done!")?;

    Ok(())
}
