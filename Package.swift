// swift-tools-version:5.5
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

let package = Package(
    name: "CooklangFind",
    platforms: [
        .iOS(.v13),
        .macOS(.v10_15)
    ],
    products: [
        .library(
            name: "CooklangFind",
            targets: ["CooklangFind", "CooklangFindFFI"]
        ),
    ],
    targets: [
        .target(
            name: "CooklangFind",
            dependencies: ["CooklangFindFFI"],
            path: "Sources/CooklangFind"
        ),
        .binaryTarget(
            name: "CooklangFindFFI",
            url: "https://github.com/cooklang/cooklang-find/releases/download/v0.8.1/CooklangFindFFI.xcframework.zip",
            checksum: "45d724bafd5d24378dc8746b8d5180e3540724aa972146ebb5f88e163877ee1f"
        ),
    ]
)
