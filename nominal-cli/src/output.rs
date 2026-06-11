use nominal::User;

pub fn print_profile_added_success(profile_name: &str, user: Option<&User>, config_path: &str) {
    if let Some(user) = user {
        println!("Authenticated as {}.", user.email());
    }
    println!("Profile '{profile_name}' saved to {config_path}.");
    println!(
        "Use this profile with `nomctl --profile {profile_name}` or `export NOMINAL_PROFILE={profile_name}`."
    );
}

pub fn print_validation_error(err: &nominal::ValidationError) {
    eprintln!("{err}");
    eprintln!("Failed to authenticate. See above for details.");
}
