// swift-tools-version: 5.10
// The native macOS front end for skillctl. It contains no skill logic of its
// own: every screen runs the `skillctl` binary with `--json` and renders the
// result, so the app and the CLI can never disagree about state. The test
// target drives the real binary end to end through the app's state layer.
import PackageDescription

let package = Package(
    name: "SkillctlApp",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(
            name: "SkillctlApp",
            path: "Sources/SkillctlApp"
        ),
        .testTarget(
            name: "SkillctlAppTests",
            dependencies: ["SkillctlApp"],
            path: "Tests/SkillctlAppTests"
        ),
    ]
)
