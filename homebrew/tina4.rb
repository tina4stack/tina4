# Copyright (c) 2026 Code Infinity
# SPDX-License-Identifier: MPL-2.0
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Tina4 < Formula
  desc "Unified CLI for the Tina4 framework — Python, PHP, Ruby, Node.js"
  homepage "https://tina4.com"
  license "MPL-2.0"
  version "3.8.114"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.114/tina4-darwin-arm64"
      sha256 "0ea037b875105e75bc34ed3623b376ab1aca962562c5e41572d360e70fdb9737"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.114/tina4-darwin-amd64"
      sha256 "617f8962187d6e35f533c7b93bb050ab00a6efe778a28ba529beffee7558680c"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.114/tina4-linux-arm64"
      sha256 "f1f0d8fa8aa6f1e8b936109dfd53ecca5832a5e0f1ac36178d2a134a9c6d36ad"
    else
      url "https://github.com/tina4stack/tina4/releases/download/v3.8.114/tina4-linux-amd64"
      sha256 "80359c4fa301d9b973c8e85e8153ccb91c31f9cac958e8c8373dc59a042a4b42"
    end
  end

  def install
    bin.install Dir["tina4*"].first => "tina4"
  end

  test do
    assert_match "tina4", shell_output("#{bin}/tina4 --version")
  end
end
