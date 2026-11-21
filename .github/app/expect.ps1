$script:expectations = 0
$script:failures = 0

function Expect([string]$Text, [bool]$Holds) {
    $script:expectations++
    if ($Holds) {
        Line "- yes: $Text"
    } else {
        $script:failures++
        Line "- NO: $Text"
    }
}

function Finish {
    Line ''
    Line "Expectations $($script:expectations), failed $($script:failures)."
    if ($script:expectations -eq 0 -or $script:failures -gt 0) { exit 1 }
    exit 0
}
