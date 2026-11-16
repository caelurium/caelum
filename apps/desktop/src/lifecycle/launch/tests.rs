use super::project_argument;

#[test]
fn project_links_and_relative_paths_reach_the_same_absolute_launch_contract() {
    let path = std::path::Path::new("/projects/café #1 & 2");
    let url = caelum_core::desktop::project_url(path).unwrap();
    assert_eq!(project_argument(&url).unwrap(), path.to_str().unwrap());
    assert!(std::path::Path::new(&project_argument(".").unwrap()).is_absolute());
    assert!(project_argument("caelum://open?path=relative").is_err());
    assert!(project_argument("caelum://open?path=/a&run=command").is_err());
    assert!(project_argument("/a\0b").is_err());
}
