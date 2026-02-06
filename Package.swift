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
            url: "https://github.com/cooklang/cooklang-find/releases/download/v0.5.6/CooklangFindFFI.xcframework.zip",
            checksum: "b4604af348707c7678ee46549f0acfd22dd0d10454eaf099f54897ecc7d9a12a"
        ),
    ]
)
