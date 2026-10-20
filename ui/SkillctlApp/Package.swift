// swift-tools-version: 5.10
// The native macOS front end for skillctl. It contains no skill logic of its
// own: every screen runs the `skillctl` binary with `--json` and renders the
// result, so the app and the CLI can never disagree about state.
import PackageDescription

let package = Package(
    name: "SkillctlApp",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(
            name: "SkillctlApp",
            path: "Sources/SkillctlApp"
        )
    ]
)
