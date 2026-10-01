use super::*;

fn convert(path: &str, case: NameCase) -> String {
    case.convert(file_name_and_stem(path).1)
}

#[test]
fn converts_file_stems_to_each_case() {
    let cases = [
        ("widget.hpp", ["Widget", "widget", "widget", "widget"]),
        (
            "src/user-service.ts",
            ["UserService", "userService", "user_service", "user-service"],
        ),
        (
            "include/user_service.hpp",
            ["UserService", "userService", "user_service", "user_service"],
        ),
        (
            "user.service.ts",
            ["UserService", "userService", "user_service", "user.service"],
        ),
        (
            "HTTPServer.ts",
            ["HttpServer", "httpServer", "http_server", "HTTPServer"],
        ),
        (
            "base64Encoder.ts",
            [
                "Base64Encoder",
                "base64Encoder",
                "base64_encoder",
                "base64Encoder",
            ],
        ),
        (
            "user-service.test.ts",
            [
                "UserServiceTest",
                "userServiceTest",
                "user_service_test",
                "user-service.test",
            ],
        ),
    ];
    for (path, expected) in cases {
        let actual = [
            NameCase::Pascal,
            NameCase::Camel,
            NameCase::Snake,
            NameCase::AsIs,
        ]
        .map(|case| convert(path, case));
        assert_eq!(actual, expected, "{path}");
    }
}

#[test]
fn splits_words_on_separators_and_case_changes() {
    assert_eq!(split_words("XMLHttpRequest"), ["XML", "Http", "Request"]);
    assert_eq!(split_words("v2Api"), ["v2", "Api"]);
    assert_eq!(split_words("__a--b__"), ["a", "b"]);
}

#[test]
fn parses_only_known_case_names() {
    for name in NameCase::NAMES {
        assert_eq!(NameCase::parse(name).unwrap().name(), name);
    }
    assert_eq!(NameCase::parse("kebab-case"), None);
}
