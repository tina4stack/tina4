# Tina4 AI skills installer for Windows.
#
# Choose a target explicitly:
#   $env:TINA4_SKILLS_TARGET = "claude"; irm https://raw.githubusercontent.com/tina4stack/tina4/main/install-skills.ps1 | iex
#   $env:TINA4_SKILLS_TARGET = "codex"; irm https://raw.githubusercontent.com/tina4stack/tina4/main/install-skills.ps1 | iex
# Use TINA4_SKILLS_TARGET=all only when both tools should receive the skills.
$ErrorActionPreference = "Stop"

# Pin skills to a released tag, not a moving branch, so an install is reproducible.
# Bump this when the skills change in a new release. Override with TINA4_SKILLS_REF.
$ref = if ($env:TINA4_SKILLS_REF) { $env:TINA4_SKILLS_REF } else { "3.13.97" }
$target = $env:TINA4_SKILLS_TARGET
$destinations = switch ($target) {
  "claude" { @(Join-Path $HOME ".claude\skills") }
  "codex"  { @(Join-Path $HOME ".agents\skills") }
  "all"    { @(Join-Path $HOME ".claude\skills"; Join-Path $HOME ".agents\skills") }
  default { throw "Set TINA4_SKILLS_TARGET to claude, codex, or all." }
}
$stage = Join-Path ([System.IO.Path]::GetTempPath()) ("tina4-skills-" + [guid]::NewGuid())

# Every file under references/, not most of them. ai-coder-rule-path.svg was
# missing here and in the sh installer, so a SUCCESSFUL install still produced
# an incomplete skill -- no error, nothing to notice.
$devRefs = @("auth-and-services.md", "data-and-orm.md", "deployment.md", "routes-and-api.md", "templates-and-frontend.md", "realtime.md", "ai-coder-rule-path.svg")

# Per-language developer skills come from their own framework repo; tina4-js and
# tina4-maintainer are shared and served from tina4-python.
$installs = @(
  @{ repo = "tina4-python"; skill = "tina4-developer-python"; refs = $devRefs }
  @{ repo = "tina4-php";    skill = "tina4-developer-php";    refs = $devRefs }
  @{ repo = "tina4-ruby";   skill = "tina4-developer-ruby";   refs = $devRefs }
  @{ repo = "tina4-nodejs"; skill = "tina4-developer-nodejs"; refs = $devRefs }
  @{ repo = "tina4-python"; skill = "tina4-js";               refs = @("html-and-components.md", "signals-and-reactivity.md", "persistence.md", "rtc.md") }
  @{ repo = "tina4-python"; skill = "tina4-maintainer";       refs = @("cli-and-deployment.md", "frond-and-frontend.md", "routing-and-orm.md", "subsystems.md") }
)
$legacySkills = @("tina4-developer")

Write-Host ""
Write-Host "  Tina4 Skills Installer" -ForegroundColor Cyan
Write-Host "  Target: $target  (ref: $ref)" -ForegroundColor Cyan
Write-Host ""

try {
  foreach ($i in $installs) {
    $base = "https://raw.githubusercontent.com/tina4stack/$($i.repo)/$ref/.claude/skills"
    $refdir = Join-Path $stage "$($i.skill)\references"
    New-Item -ItemType Directory -Path $refdir -Force | Out-Null
    Invoke-WebRequest -Uri "$base/$($i.skill)/SKILL.md" -OutFile (Join-Path $stage "$($i.skill)\SKILL.md")
    foreach ($reference in $i.refs) {
      Invoke-WebRequest -Uri "$base/$($i.skill)/references/$reference" -OutFile (Join-Path $refdir $reference)
    }
    Write-Host "  + $($i.skill)  ($($i.repo))" -ForegroundColor Green
  }

  foreach ($destination in $destinations) {
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    foreach ($legacySkill in $legacySkills) {
      $legacyPath = Join-Path $destination $legacySkill
      if (Test-Path -LiteralPath $legacyPath) {
        Remove-Item -LiteralPath $legacyPath -Recurse -Force
        Write-Host "  - removed legacy $legacySkill" -ForegroundColor DarkYellow
      }
    }
    foreach ($skill in Get-ChildItem -Path $stage -Directory) {
      $replacement = Join-Path $destination ("." + $skill.Name + ".tina4-new")
      $installed = Join-Path $destination $skill.Name
      Remove-Item -Path $replacement -Recurse -Force -ErrorAction SilentlyContinue
      Copy-Item -Path $skill.FullName -Destination $replacement -Recurse -Force
      Remove-Item -Path $installed -Recurse -Force -ErrorAction SilentlyContinue
      Move-Item -Path $replacement -Destination $installed -Force
    }
    Set-Content -Path (Join-Path $destination ".tina4-skills-ref") -Value $ref -NoNewline
    Write-Host "  installed for $destination" -ForegroundColor Green
  }
} finally {
  Remove-Item -Path $stage -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "  Done - six skills installed for $target (ref $ref). Restart your coding tool to pick them up." -ForegroundColor Green

# SIG # Begin signature block
# MIIoOAYJKoZIhvcNAQcCoIIoKTCCKCUCAQExDzANBglghkgBZQMEAgEFADB5Bgor
# BgEEAYI3AgEEoGswaTA0BgorBgEEAYI3AgEeMCYCAwEAAAQQH8w7YFlLCE63JNLG
# KX7zUQIBAAIBAAIBAAIBAAIBADAxMA0GCWCGSAFlAwQCAQUABCDiQK0YFZMFM9PH
# 6phwbjTcnLYs91L2Q3S/sqIDiymCzaCCINgwggXJMIIEsaADAgECAhAbtY8lKt8j
# AEkoya49fu0nMA0GCSqGSIb3DQEBDAUAMH4xCzAJBgNVBAYTAlBMMSIwIAYDVQQK
# ExlVbml6ZXRvIFRlY2hub2xvZ2llcyBTLkEuMScwJQYDVQQLEx5DZXJ0dW0gQ2Vy
# dGlmaWNhdGlvbiBBdXRob3JpdHkxIjAgBgNVBAMTGUNlcnR1bSBUcnVzdGVkIE5l
# dHdvcmsgQ0EwHhcNMjEwNTMxMDY0MzA2WhcNMjkwOTE3MDY0MzA2WjCBgDELMAkG
# A1UEBhMCUEwxIjAgBgNVBAoTGVVuaXpldG8gVGVjaG5vbG9naWVzIFMuQS4xJzAl
# BgNVBAsTHkNlcnR1bSBDZXJ0aWZpY2F0aW9uIEF1dGhvcml0eTEkMCIGA1UEAxMb
# Q2VydHVtIFRydXN0ZWQgTmV0d29yayBDQSAyMIICIjANBgkqhkiG9w0BAQEFAAOC
# Ag8AMIICCgKCAgEAvfl4+ObVgAxknYYblmRnPyI6HnUBfe/7XGeMycxca6mR5rlC
# 5SBLm9qbe7mZXdmbgEvXhEArJ9PoujC7Pgkap0mV7ytAJMKXx6fumyXvqAoAl4Va
# qp3cKcniNQfrcE1K1sGzVrihQTib0fsxf4/gX+GxPw+OFklg1waNGPmqJhCrKtPQ
# 0WeNG0a+RzDVLnLRxWPa52N5RH5LYySJhi40PylMUosqp8DikSiJucBb+R3Z5yet
# /5oCl8HGUJKbAiy9qbk0WQq/hEr/3/6zn+vZnuCYI+yma3cWKtvMrTscpIfcRnNe
# GWJoRVfkkIJCu0LW8GHgwaM9ZqNd9BjuiMmNF0UpmTJ1AjHuKSbIawLmtWJFfzcV
# WiNoidQ+3k4nsPBADLxNF8tNorMe0AZa3faTz1d1mfX6hhpneLO/lv403L3nUlbl
# s+V1e9dBkQXcXWnjlQ1DufyDljmVe2yAWk8TcsbXfSl6RLpSpCrVQUYJIP4ioLZb
# MI28iQzV13D4h1L92u+sUS4Hs07+0AnacO+Y+lbmbdu1V0vc5SwlFcieLnhO+Nqc
# noYsylfzGuXIkosagpZ6w7xQEmnYDlpGizrrJvojybawgb5CAKT41v4wLsfSRvbl
# jnX98sy50IdbzAYQYLuDNbdeZ95H7JlI8aShFf6tjGKOOVVPORa5sWOd/7cCAwEA
# AaOCAT4wggE6MA8GA1UdEwEB/wQFMAMBAf8wHQYDVR0OBBYEFLahVDkCw6A/joq8
# +tT4HKbROg79MB8GA1UdIwQYMBaAFAh2zcsH/yT2xc3tu5C84oQ3RnX3MA4GA1Ud
# DwEB/wQEAwIBBjAvBgNVHR8EKDAmMCSgIqAghh5odHRwOi8vY3JsLmNlcnR1bS5w
# bC9jdG5jYS5jcmwwawYIKwYBBQUHAQEEXzBdMCgGCCsGAQUFBzABhhxodHRwOi8v
# c3ViY2Eub2NzcC1jZXJ0dW0uY29tMDEGCCsGAQUFBzAChiVodHRwOi8vcmVwb3Np
# dG9yeS5jZXJ0dW0ucGwvY3RuY2EuY2VyMDkGA1UdIAQyMDAwLgYEVR0gADAmMCQG
# CCsGAQUFBwIBFhhodHRwOi8vd3d3LmNlcnR1bS5wbC9DUFMwDQYJKoZIhvcNAQEM
# BQADggEBAFHCoVgWIhCL/IYx1MIy01z4S6Ivaj5N+KsIHu3V6PrnCA3st8YeDrJ1
# BXqxC/rXdGoABh+kzqrya33YEcARCNQOTWHFOqj6seHjmOriY/1B9ZN9DbxdkjuR
# mmW60F9MvkyNaAMQFtXx0ASKhTP5N+dbLiZpQjy6zbzUeulNndrnQ/tjUoCFBMQl
# lVXwfqefAcVbKPjgzoZwpic7Ofs4LphTZSJ1Ldf23SIikZbr3WjtP6MZl9M7JYjs
# NhI9qX7OAo0FmpKnJ25FspxihjcNpDOO16hO0EoXQ0zF8ads0h5YbBRRfopUofbv
# n3l6XYGaFpAP4bvxSgD5+d2+7arszgowggaCMIIEaqADAgECAhAo8HfBHDa9/l90
# MkdwJy4DMA0GCSqGSIb3DQEBDAUAMFYxCzAJBgNVBAYTAlBMMSEwHwYDVQQKExhB
# c3NlY28gRGF0YSBTeXN0ZW1zIFMuQS4xJDAiBgNVBAMTG0NlcnR1bSBUaW1lc3Rh
# bXBpbmcgMjAyMSBDQTAeFw0yNjAzMTEwNzM0NTRaFw0zNjAyMjcwNzM0NTRaMFAx
# CzAJBgNVBAYTAlBMMSEwHwYDVQQKDBhBc3NlY28gRGF0YSBTeXN0ZW1zIFMuQS4x
# HjAcBgNVBAMMFUNlcnR1bSBUaW1lc3RhbXAgMjAyNjCCAiIwDQYJKoZIhvcNAQEB
# BQADggIPADCCAgoCggIBALi9zfBw+xI12hi2F9uNQBNOSTkTIi13fuh3DiWybI53
# 92bBfJw6tO7zDs7UqP/rejVf4xFTU3XNOPsuOFkP5SPFsnWwn3VJL+NhNhsuujvY
# sQtaQnvNo1wk46reTgws1L8OFEdKaxgyqT4zOQx81mLwNZSqXNU9Rrb9oAJWJF4M
# ydz+C1ebu0D0+vgM3tqAQ8TU203fqgKCPqIdHloaFqH9eyR+8tnpmy2hnnN59ZOt
# msuHeRwf5iTOhlhf0d/qqquLOSIGPgGVL1Hj5XBUox2OE7tQRr+l/xxNbL6OuwBO
# 4Aa2EbuxYg37cpXv5i2/lTL4NASB/duf9W4fjA3Ro+GP+bC54n2OYiM3Cq8z92ej
# x8F5HWO2e4/75eC95B9tMw/eq7109GDyW2+2ut5Bn0o8MXJTERkyystWbqcnUkom
# A97qfiDCwvpWuOMdfTOksXe9S+xDxOJaCbiyTivgPulxcT+tfQTRXBr5UwtPvc+I
# NnafOEilzVHA11tv+pHcylryxBc08cUihf/ttqkZS0bmx3VxQay8uohaTp0//ECA
# 9vYGW51SKdJMJhXvKyIdBLy8WYNchTt8yRITv9Adxg9JqGujh+TnP1bW3aQjFtlE
# l7SzgNeI8OVA/CJAWWUuRT3Au9ToOpHy/flrrknhN4heEw7/6ILMv4HSsj0YZ4ot
# AgMBAAGjggFQMIIBTDB1BggrBgEFBQcBAQRpMGcwOwYIKwYBBQUHMAKGL2h0dHA6
# Ly9zdWJjYS5yZXBvc2l0b3J5LmNlcnR1bS5wbC9jdHNjYTIwMjEuY2VyMCgGCCsG
# AQUFBzABhhxodHRwOi8vc3ViY2Eub2NzcC1jZXJ0dW0uY29tMB8GA1UdIwQYMBaA
# FL5UAi+/QGxzQ86sCSVOnkNEGu7gMAwGA1UdEwEB/wQCMAAwOQYDVR0fBDIwMDAu
# oCygKoYoaHR0cDovL3N1YmNhLmNybC5jZXJ0dW0ucGwvY3RzY2EyMDIxLmNybDAW
# BgNVHSUBAf8EDDAKBggrBgEFBQcDCDAOBgNVHQ8BAf8EBAMCB4AwIgYDVR0gBBsw
# GTAIBgZngQwBBAIwDQYLKoRoAYb2dwIFAQswHQYDVR0OBBYEFCM5aiirmhKnpKfy
# H/lFmr+N/oIDMA0GCSqGSIb3DQEBDAUAA4ICAQBm/pObrAveTqYFPCSthxNVwBHT
# W4TfuBQY2KGCdMlH6ALjVPYf2XARNGMTXScR0JQ2/c2LxLpoHHhRwxFf3d933DhF
# S165rYj6iPSQKm41rc9AMpTCjMdmai1aK9o870pOzTzLRhWFVIxm+5Qf+t4V5z14
# tPoUsAWdfRKD1Zzvzfe5AHlE9rxOZcGROiULxs3Edf7ZJ8RZI0jzD9juj2oEX2XS
# 7BVKuNCK1W0h1mjapv028yqo8UtZh5/1Ib48Yyp88BDYShh5PSGUUuS5tYXVfcdu
# IPBe3PdNTYRYdT5VNxkwLgjMzIcr/6SH2fejFJDjnCxjvXLuarErJg/IdI+hXdGI
# BDVOGv1zrkF4ktoQvFZIp5DIAYh0GwyUhc87qqn5UaYTj2+iyiBDSNh7aGCrkuAn
# qg7Qh7frhX+QhMGeJ9SpLpnG76qN85hLyjvYR6gRYiLTj8G4fxjqV400/buPw0rB
# o9yJZyq/8cGdCOLPOYf9qpefTcrZKeO9pg+UJHx8UABbaTqizRxm8sUSEycccCCF
# hIit3AI/3wGuSOADeiA676NcQ7+dsP04YvzvdN7pTZNyg672WpBDJdg0eItHl4yo
# mvNeIOWh1qs4QqOW6TI8Ocsi4uSDTLorxLUDnBTwKQzzpxd1YUctEgRh32R0J9Fk
# FkeEruJU94RljFr1uDCCBrkwggShoAMCAQICEQDn/2nHOzXOS5Em2HR8aKWHMA0G
# CSqGSIb3DQEBDAUAMIGAMQswCQYDVQQGEwJQTDEiMCAGA1UEChMZVW5pemV0byBU
# ZWNobm9sb2dpZXMgUy5BLjEnMCUGA1UECxMeQ2VydHVtIENlcnRpZmljYXRpb24g
# QXV0aG9yaXR5MSQwIgYDVQQDExtDZXJ0dW0gVHJ1c3RlZCBOZXR3b3JrIENBIDIw
# HhcNMjEwNTE5MDUzMjA3WhcNMzYwNTE4MDUzMjA3WjBWMQswCQYDVQQGEwJQTDEh
# MB8GA1UEChMYQXNzZWNvIERhdGEgU3lzdGVtcyBTLkEuMSQwIgYDVQQDExtDZXJ0
# dW0gVGltZXN0YW1waW5nIDIwMjEgQ0EwggIiMA0GCSqGSIb3DQEBAQUAA4ICDwAw
# ggIKAoICAQDpEh8ENe25XXrFppVBvoplf0530W0lddNmjtv4YSh/f7eDQKFaIqc7
# tHj7ox+u8vIsJZlroakUeMS3i3T8aJRC+eQs4FF0GqvkM6+WZO8kmzZfxmZaBYmM
# Ls8FktgFYCzywmXeQ1fEExflee2OpbHVk665eXRHjH7MYZIzNnjl2m8Hy8ulB9mR
# 8wL/W0v0pjKNT6G0sfrx1kk+3OGosFUb7yWNnVkWKU4qSxLv16kJ6oVJ4BSbZ4xM
# ak6JLeB8szrK9vwGDpvGDnKCUMYL3NuviwH1x4gZG0JAXU3x2pOAz91JWKJSAmRy
# /l0s0l5bEYKolg+DMqVhlOANd8Yh5mkQWaMEvBRE/kAGzIqgWhwzN2OsKIVtO8mf
# 5sPWSrvyplSABAYa13rMYnzwfg08nljZHghquCJYCa/xHK9acev9UD7Y+usr15d7
# mrszzxhF1JOr1Mpup2chNSBlyOObhlSO16rwrffVrg/SzaKfSndS5swRhr8bnDqN
# JY9TNyEYvBYpgF95K7p0g4LguR4A++Z1nFIHWVY5v0fNVZmgzxD9uVo/gta3onGO
# Qj3JCxgYx0KrCXu4yc9QiVwTFLWbNdHFSjBCt5/8Q9pLuRhVocdCunhcHudMS1CG
# Q/Rn0+7P+fzMgWdRKfEOh/hjLrnQ8BdJiYrZNxvIOhM2aa3zEDHNwwIDAQABo4IB
# VTCCAVEwDwYDVR0TAQH/BAUwAwEB/zAdBgNVHQ4EFgQUvlQCL79AbHNDzqwJJU6e
# Q0Qa7uAwHwYDVR0jBBgwFoAUtqFUOQLDoD+Oirz61PgcptE6Dv0wDgYDVR0PAQH/
# BAQDAgEGMBMGA1UdJQQMMAoGCCsGAQUFBwMIMDAGA1UdHwQpMCcwJaAjoCGGH2h0
# dHA6Ly9jcmwuY2VydHVtLnBsL2N0bmNhMi5jcmwwbAYIKwYBBQUHAQEEYDBeMCgG
# CCsGAQUFBzABhhxodHRwOi8vc3ViY2Eub2NzcC1jZXJ0dW0uY29tMDIGCCsGAQUF
# BzAChiZodHRwOi8vcmVwb3NpdG9yeS5jZXJ0dW0ucGwvY3RuY2EyLmNlcjA5BgNV
# HSAEMjAwMC4GBFUdIAAwJjAkBggrBgEFBQcCARYYaHR0cDovL3d3dy5jZXJ0dW0u
# cGwvQ1BTMA0GCSqGSIb3DQEBDAUAA4ICAQC4k1l3yUwV/ZQHCKCneqAs8EGTnwEU
# JLdDpokN/dMhKjK0rR5qX8nIIHzxpQR3TAw2IRw1Uxsr2PliG3bCFqSdQTUbfaTq
# 6V3vBzEebDru9QFjqlKnxCF2h1jhLNFFplbPJiW+JSnJTh1fKEqEdKdxgl9rVTvl
# xfEJ7exOn25MGbd/wGPwuSmMxRJVO0wnqgS7kmoJjNF9zqeehFSDDP8ZVkWg4EZ2
# tIS0M3uZmByRr+1Lkwjjt8AtW83mVnZTyTsOb+FNfwJY7DS4FmWhkRbgcHRetreo
# TirPOr/ozyDKhT8MTSTf6Lttg6s6T/u08mDWw6HK04ZRDfQ9sb77QV8mKgO44WGP
# 31vXnVKoWVJpFBjPvjL8/Zck/5wXX2iqjOaLStFOR/IQki+Ehn4zlcgVm22ZVCBP
# F+l8nAwUUShCtKuSU7GmZLKCmmxQMkSiWILTm8EtVD6AxnJhoq8EnhjEEyUoflke
# RF2WhFiVQOmWTwZRr44IxWGkNJC6tTorW5rl2Zl+2e9JLPYf3pStAPMDoPKIjVXd
# 6NW2+fZrNUBeDo2eOa5Fn7Brs/HLQff5Xgris5MeUbdVgDrF8uxO6cLPvZPo63j6
# 2SsNg55pTWk9fUIF9iPoRbb4QurjoY/woI1RAOKtYtTic6aAJq3u83RIPpGXBSJK
# wx4KJAOZnCDCtTCCBs0wggS1oAMCAQICEQC78My1t7gx/SGuMneK5AyJMA0GCSqG
# SIb3DQEBDAUAMIGAMQswCQYDVQQGEwJQTDEiMCAGA1UEChMZVW5pemV0byBUZWNo
# bm9sb2dpZXMgUy5BLjEnMCUGA1UECxMeQ2VydHVtIENlcnRpZmljYXRpb24gQXV0
# aG9yaXR5MSQwIgYDVQQDExtDZXJ0dW0gVHJ1c3RlZCBOZXR3b3JrIENBIDIwHhcN
# MjEwNTE5MDUzMjEzWhcNMzYwNTE4MDUzMjEzWjBqMQswCQYDVQQGEwJQTDEhMB8G
# A1UEChMYQXNzZWNvIERhdGEgU3lzdGVtcyBTLkEuMTgwNgYDVQQDEy9DZXJ0dW0g
# RXh0ZW5kZWQgVmFsaWRhdGlvbiBDb2RlIFNpZ25pbmcgMjAyMSBDQTCCAiIwDQYJ
# KoZIhvcNAQEBBQADggIPADCCAgoCggIBAJS++PCWqN/1551FMr42hq9mVXGZBM8g
# 3WcrldBFRBbI9bAcPnjBrZoPaREOsCMF/vX9PRc306qazkwv8miWCvK/clbv/JqG
# nuWD/Cfc8Oc3DMLUGUUzIJ0SS1CZRcyYTiabFK61FjU4YZqAQ1Y+NtDVH/Hge5MH
# JZsb5NRXt0g/ETtMzL02lyjh/T0M3pAVFd2TttlO7nyTG7owUfqbNCQLche8TF+j
# Rz8JPj+FNkPZeSceNcvOL2PMidIdL0ZCp5d3qdZuTAS7vOcqk0RxISWisNIRxGFz
# U5I4uNQrTHRw7DzCHNDR9YaQ+iqAKyVuNmiSjtTuSP8WfhNZDyzvkseOFVqj7DZJ
# Yw/QQtZgiSrUKiYarfCoEPtVTqu0IOhR/bfseSBEHEtcWPCnjjigpadyFgxnt6lZ
# fyOqe1KvhAd6J0vvZzpezBAghmwWCVXgE7byM7TbJLvDiLJjRzo9CzfgZntJamGw
# 3UNqJf3KxzqGi9VihUD3i37enhMjVG0r0DKHRq2Z9g8mq0F+TmQnWHic1rHSVmYk
# v1yEmNlBv7tdbwxCCy2JB2x6BrPZAvoR2b73g3whbpNhpqXI70t5HfkD0yhMujv2
# t0HTidpOpZ09MEqHsOlLndkBCfl1izal6v6btl4jGcRZo1wb49tGH5myUVvozvkm
# A3jezMH79VhVAgMBAAGjggFVMIIBUTAPBgNVHRMBAf8EBTADAQH/MB0GA1UdDgQW
# BBSsV8oIFtw/xTEcCk3b+/HemSctNDAfBgNVHSMEGDAWgBS2oVQ5AsOgP46KvPrU
# +Bym0ToO/TAOBgNVHQ8BAf8EBAMCAQYwEwYDVR0lBAwwCgYIKwYBBQUHAwMwMAYD
# VR0fBCkwJzAloCOgIYYfaHR0cDovL2NybC5jZXJ0dW0ucGwvY3RuY2EyLmNybDBs
# BggrBgEFBQcBAQRgMF4wKAYIKwYBBQUHMAGGHGh0dHA6Ly9zdWJjYS5vY3NwLWNl
# cnR1bS5jb20wMgYIKwYBBQUHMAKGJmh0dHA6Ly9yZXBvc2l0b3J5LmNlcnR1bS5w
# bC9jdG5jYTIuY2VyMDkGA1UdIAQyMDAwLgYEVR0gADAmMCQGCCsGAQUFBwIBFhho
# dHRwOi8vd3d3LmNlcnR1bS5wbC9DUFMwDQYJKoZIhvcNAQEMBQADggIBALuSGVmM
# VtT6CxF1ytoUUkqPU8vrcxW52eb8Agu41rGbsshk5i2tlpFYSlHSp3e6FKlQQjb1
# 8wFm+67cIdcmwznyFskWjhigefIMmBOR5WZzXR93U0j/9wjedEPX8CXHT3JaE9mX
# N7QD8G/Bzctxm1PLB+wvm0PdAAvxmp6Wse3URvwCcMScDMo1ubWbBiYP/l3LVeQg
# p+wzWmZEzZkNwniqnBV7ATuD7h75j6KrjYu5oc+80CTCrR1oD0enep6aTRxEV8zn
# bnsy8Sy77rDz7jialBQ6CkBJUiNjeXa91/oFR8El+x3elyn/aF8OddqqX0CgHumz
# t4Jlv1Vcqb+7aVNEx/l6w5xkXLi52ycYFo7BJJR7RyFi6x8n0iOyamgyXgJbkTRH
# 9TnkHPpOkOj6BpdAkviL/O/RRHmlqkawOC4EBKn2D85+e8ZN5atL2XmisSS270TG
# sV8+8ArFkr7gA71tqk+EwBOtoqjqTHornWlLVk7al/WvcH6Vc3ltMuCnyD93KBRm
# bZyZq4EDMkDm/+Ic5vRvhvgy6pzx3GBxfWNlEu0u/GcBbGQogx+PsDgzQms3issF
# Sk8UzT4GnaxHuu0Wo1lbnqG5oOTE7vl41E3bPgD3n1HkYFGcVa2ATrn4HVEcBBD9
# M4XiZTRjegiFVCvetSHi11K8jGvxCmwMoi+wMIIG8zCCBNugAwIBAgIQUh2Iv33J
# FZ7jRFhh2xJhxjANBgkqhkiG9w0BAQsFADBqMQswCQYDVQQGEwJQTDEhMB8GA1UE
# ChMYQXNzZWNvIERhdGEgU3lzdGVtcyBTLkEuMTgwNgYDVQQDEy9DZXJ0dW0gRXh0
# ZW5kZWQgVmFsaWRhdGlvbiBDb2RlIFNpZ25pbmcgMjAyMSBDQTAeFw0yNTA3MDgx
# MDMzMjNaFw0yNzA3MDgxMDMzMjJaMIH9MR0wGwYDVQQPExRQcml2YXRlIE9yZ2Fu
# aXphdGlvbjETMBEGCysGAQQBgjc8AgEDEwJaQTEXMBUGA1UEBRMOMjAxNC8wMDM4
# NjYvMDcxCzAJBgNVBAYTAlpBMRUwEwYDVQQIDAxXZXN0ZXJuIENhcGUxEjAQBgNV
# BAcMCUNhcGUgVG93bjENMAsGA1UEERMENzU3MDEjMCEGA1UECQwaMUIgTWFkcmlk
# IFN0cmVldCwgVWl0emljaHQxIDAeBgNVBAoMF0NvZGUgSW5maW5pdHkgKFB0eSkg
# THRkMSAwHgYDVQQDDBdDb2RlIEluZmluaXR5IChQdHkpIEx0ZDCCAaIwDQYJKoZI
# hvcNAQEBBQADggGPADCCAYoCggGBANbvUpERu2NwodnGGxZr/bbzD0MYo3OoSGpq
# PNBOO5AWpL6Fq8l+PoAMBZ2bEvAk/TMDNQyrFZ7zrmQZRJhxfWLkEIurPFo4hFfp
# 3iq70iuwwAMYIJidKJKJfFm8aznY2QTTh8SE/ThEgzuq5EGbT4fbhPIde97U4KoC
# nOW1EMh5BrXA/qrVD24+1A4hQ5704+xIzLQaGuPpMo9Xl7/4kslhuTVDhQg2r4ay
# +feK47hB3D8EXBfANzaCfRk2j5Xd1m5sMgbnf6tspkiybtA//bAHiwP05a3snQZr
# 264GaGcLE8EwWgJYxDDhIqU0yOZuYnR9mT2YCwuMDLNLeE+ADzfRujDZhmPG5Qei
# 3gBp9q2riIgcGLLZbQCOY7GAk7f9Wp91P8YiDWZc0/YPihyYcyCobhhz4iKI62CG
# cVlQCAOhPw+bqCHckrT9GgAMBZ94NrVFw/W1n+XRpve40BjSEyDPsbjQDQk8JMGh
# 5rF1AIIfmWeCLqpNb/DKEm2NLGQB2wIDAQABo4IBfzCCAXswDAYDVR0TAQH/BAIw
# ADBBBgNVHR8EOjA4MDagNKAyhjBodHRwOi8vY2V2Y3NjYTIwMjEuY3JsLmNlcnR1
# bS5wbC9jZXZjc2NhMjAyMS5jcmwwdwYIKwYBBQUHAQEEazBpMC4GCCsGAQUFBzAB
# hiJodHRwOi8vY2V2Y3NjYTIwMjEub2NzcC1jZXJ0dW0uY29tMDcGCCsGAQUFBzAC
# hitodHRwOi8vcmVwb3NpdG9yeS5jZXJ0dW0ucGwvY2V2Y3NjYTIwMjEuY2VyMB8G
# A1UdIwQYMBaAFKxXyggW3D/FMRwKTdv78d6ZJy00MB0GA1UdDgQWBBRiwGM3zjka
# tT29wM2wBQGaWH4wajBKBgNVHSAEQzBBMAcGBWeBDAEDMDYGCyqEaAGG9ncCBQEH
# MCcwJQYIKwYBBQUHAgEWGWh0dHBzOi8vd3d3LmNlcnR1bS5wbC9DUFMwEwYDVR0l
# BAwwCgYIKwYBBQUHAwMwDgYDVR0PAQH/BAQDAgeAMA0GCSqGSIb3DQEBCwUAA4IC
# AQARziix3C4lREs8oRFu2oaTqAPTNKNXhA/kA7KPj2HmLlrpiPkx/WB3wV5uy0ca
# Nga/extnvhjksmfGLW3MYE+ThUF5h3Q0U8pXD9pZUKlWKLemsbggt4cK3aWNX5dJ
# l6LY1dNwMmuP4G64TJPsiwl4TpoTuL+rIRjPSuL34mUR/87Vc1eEcjhKYyQzk5Po
# 04RAPUa/9hkNz5bvfNQKf0374ZjwQ34J3FKQ/hJvEU+ifiosclXVwJy5491y/pA/
# CnLBLXriyHnavsC3v3TWkecSoTTORd1TfEvmTTd+zbkPJiuN43OjOJdt7trm0X1m
# iz/k8k3qsjrBJg8VEvKh2uVg3B4Im9U40xXGGPcIAavOgwKkt9BANoAgoY526HPh
# 8T0emd3vDomkLN7h5b5QEAYZnzbvXnKUnIKMHcnmIEGt9IzCEzmHoHBuiLQKgnyF
# 5gzF+F+Fel5ulonDwgjGb4bSKtKFqRabKlQb750kNtz/s3Q4/b7o/iHnFZQKluqW
# AqZDM/oSfXtIVsIQzMmvCU/n9h82zZYggEZN/5TeRAe/Kz7vRqtPphsFO1Ko3qkv
# Oluu8JV6gVSS5u0X48nYRmiE4FG8tlbicUjM4tZ69X9RCOC2nsmADu0SbrcXF+ZB
# ZWa0p0z2y8MnNZ3wa+xhqveZDGqWS/IyiZWH2KH9STsLqzGCBrYwggayAgEBMH4w
# ajELMAkGA1UEBhMCUEwxITAfBgNVBAoTGEFzc2VjbyBEYXRhIFN5c3RlbXMgUy5B
# LjE4MDYGA1UEAxMvQ2VydHVtIEV4dGVuZGVkIFZhbGlkYXRpb24gQ29kZSBTaWdu
# aW5nIDIwMjEgQ0ECEFIdiL99yRWe40RYYdsSYcYwDQYJYIZIAWUDBAIBBQCggYQw
# GAYKKwYBBAGCNwIBDDEKMAigAoAAoQKAADAZBgkqhkiG9w0BCQMxDAYKKwYBBAGC
# NwIBBDAcBgorBgEEAYI3AgELMQ4wDAYKKwYBBAGCNwIBFTAvBgkqhkiG9w0BCQQx
# IgQg+ln8XEeSuy2dbYwye8qmRxFZ4AcHCokEhOw7K/raeOwwDQYJKoZIhvcNAQEB
# BQAEggGATKICHVJCGhLVQcpuf0kYP435mw5rftF0N+RfY54VcC0C0wH5zbSPauXb
# +Lf3jQzdwZO8ibcS3G6dg/VpRMrdmPiXpAMI+sUBVxKE4WcAa/fVkzBaYVAlMKc0
# cCZrWW1qZgWw0VREfKQRTUbj0fgkWwpaSkpfXY0n6JCjoO9xHXo9rERYoEk8Ca8H
# Ec4QVh9Zit0madNLApdI2J4IyXu/aBIZaqHRxiDJDS8rE+M9RzPp5B+Vk39rUgAo
# fFER9SoDviA2V4+U6KWy/k3WsZdrMp1mIPcxlHNMV90h0zZT9CB7wv0CwDeDXt+1
# 5mJknzYxtjp9dN6xzl/WBiR6o0b91REWviXnekaeB82SklhJ1hi36/UO3KZMkG8E
# VtjlqcgzMmcjgyOMdDqfPqFBzQWrkFV2H8rSVcHylKMTqX1Xl+QF4uh7CZ8Ogtd0
# 4Hy7Xr7ERXs8S7ngW32FhhR5Ezkp8Ny6NMGuUQr459SUgvtVc/yawmHypr3SPcJe
# J92HtJsioYIEAjCCA/4GCSqGSIb3DQEJBjGCA+8wggPrAgEBMGowVjELMAkGA1UE
# BhMCUEwxITAfBgNVBAoTGEFzc2VjbyBEYXRhIFN5c3RlbXMgUy5BLjEkMCIGA1UE
# AxMbQ2VydHVtIFRpbWVzdGFtcGluZyAyMDIxIENBAhAo8HfBHDa9/l90MkdwJy4D
# MA0GCWCGSAFlAwQCAgUAoIIBVjAaBgkqhkiG9w0BCQMxDQYLKoZIhvcNAQkQAQQw
# HAYJKoZIhvcNAQkFMQ8XDTI2MDgwNDEyMzE1NlowNwYLKoZIhvcNAQkQAi8xKDAm
# MCQwIgQghb6Q4QrSQ418ySi2r0iwmrIIF3zs+LASbFjTkQUlxDwwPwYJKoZIhvcN
# AQkEMTIEMAIuHYx9MimjdeWbjSOiMAodN8JiFLjvcwNM3vcN016qNz1Rd9uIK3cG
# JT2U2/LJvzCBnwYLKoZIhvcNAQkQAgwxgY8wgYwwgYkwgYYEFFcUaEEMqFrzQk75
# FkpRNhD0042YMG4wWqRYMFYxCzAJBgNVBAYTAlBMMSEwHwYDVQQKExhBc3NlY28g
# RGF0YSBTeXN0ZW1zIFMuQS4xJDAiBgNVBAMTG0NlcnR1bSBUaW1lc3RhbXBpbmcg
# MjAyMSBDQQIQKPB3wRw2vf5fdDJHcCcuAzANBgkqhkiG9w0BAQEFAASCAgBA6DV6
# /wTvUWqBXlXECWEzm97xLkQxWL1nimNlcztp0e7o3u3JfQlgEWoWUsSruRc1tr56
# I5cT/PVgnHRF4w4kryzv45uCzY43TpHue7zs14cBpvFALXC2v80CRb1JrRQ+ykHW
# XF+K82fGjhtDBLesoTOMoj/gCLfev2iBGp43GLdsXWZFgq6GD3r3fiDBiAFQTt3w
# k/wU/H9AU3o5kX3ZdwFzPjLmBxsAMakMd22HIqwCcAHuS7ZYePTVVf/s/+i+g0U0
# 8bDA7o/7HpAHl7bV1KjLsWPTPjPwBTsc4GD9S1LnC/qgPON2bI+zDkSf3uhPHOEY
# R/kb6TOUXeS3AP3T3APEMCtudQBvV1QYE/DWgKaLUjKbE7ifGn6QZ3hH60VXEDlo
# wUgXKC5gble5Rca9o6hyS3Lj+hMXa0wNqBGLMV/gUBjyg5V1bWxrFWyXwBc0rcsx
# DTNt6poB5oGlBYR+pPJiBYJoRiWKJOZgGwfC8pVs7viiLswPsI4bef4D9/jFmp5W
# JctJ8LMnfpSGAvoPB7OcuMJwxgTq81LcucyhCT2DVeJGc52nR54F/RrjZWR1+Lk4
# KFCp0o1pve5JG4j1v1PvKV0RbABzMa++SD6Kptc+hR2/RPbsWP9OR5n16p9T9cWe
# GB55yE3nlx/bLcmTMJg/CLND+6fg8OI/hOW3jg==
# SIG # End signature block
