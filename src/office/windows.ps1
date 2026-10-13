# Invoked by pdfcli with Windows PowerShell 5.1 in an STA process.
# Environment variables carry paths without evaluating them as PowerShell.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$appName = $env:PDFCLI_OFFICE_APP
$inputPath = $env:PDFCLI_OFFICE_INPUT
$outputPath = $env:PDFCLI_OFFICE_OUTPUT
$recoveryPath = $env:PDFCLI_OFFICE_RECOVERY
$application = $null
$documents = $null
$document = $null
$mutex = $null
$locked = $false
$state = $null
$mayQuit = $false
$failure = $null
$cleanupOk = $true

try {
    if ($appName -notin @('Word', 'Excel', 'PowerPoint')) {
        throw 'Unknown Microsoft Office application'
    }
    $comType = [Type]::GetTypeFromProgID("$appName.Application", $false)
    if ($null -eq $comType) {
        # Reserved for absence, never a generic conversion failure.
        exit 42
    }

    # PowerPoint can share a running instance; serialize app-wide settings.
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $mutex = New-Object Threading.Mutex($false, "Local\pdfcli-office-$sid")
    try {
        $locked = $mutex.WaitOne(0)
    } catch [Threading.AbandonedMutexException] {
        $locked = $true
    }
    if (-not $locked) { throw 'Another pdfcli Office conversion is running' }

    # Word and Excel create separate automation instances. PowerPoint may reuse
    # an existing process, which must never be quit by this worker.
    $powerPointWasRunning = @(Get-Process -Name POWERPNT -ErrorAction SilentlyContinue).Count -gt 0
    $application = [Activator]::CreateInstance($comType)
    $mayQuit = ($appName -ne 'PowerPoint') -or (-not $powerPointWasRunning)
    switch ($appName) {
        'Word' { $documents = $application.Documents }
        'Excel' { $documents = $application.Workbooks }
        'PowerPoint' { $documents = $application.Presentations }
    }
    $state = [ordered]@{
        Application = $appName
        StagedInput = $inputPath
        AutomationSecurity = $application.AutomationSecurity
        DisplayAlerts = $application.DisplayAlerts
    }
    if ($appName -eq 'Excel') { $state.EnableEvents = $application.EnableEvents }
    # Retain original settings if an unresponsive COM call prevents finally.
    $state | ConvertTo-Json | Set-Content -LiteralPath $recoveryPath -Encoding UTF8
    $application.AutomationSecurity = 3 # msoAutomationSecurityForceDisable
    switch ($appName) {
        'Word' { $application.DisplayAlerts = 0 }
        'Excel' {
            $application.DisplayAlerts = $false
            $application.EnableEvents = $false
        }
        'PowerPoint' { $application.DisplayAlerts = 1 } # ppAlertsNone
    }

    $missing = [Type]::Missing
    switch ($appName) {
        'Word' {
            # Read-only, no recent-file entry, no conversion dialog, hidden.
            $document = $documents.Open($inputPath, $false, $true, $false,
                '', '', $false, '', '', $missing, $missing, $false)
            $document.ExportAsFixedFormat($outputPath, 17, $false) # wdExportFormatPDF
        }
        'Excel' {
            # Disable link updates, open read-only, ignore read-only advice,
            # suppress password prompting and omit the recent-file entry.
            $document = $documents.Open($inputPath, 0, $true, $missing,
                '', '', $true, $missing, $missing, $false, $false, $missing, $false)
            # Export the workbook, honoring saved print areas and pagination.
            $document.ExportAsFixedFormat(0, $outputPath, 0, $false, $false,
                $missing, $missing, $false) # xlTypePDF, standard quality
        }
        'PowerPoint' {
            $document = $documents.Open($inputPath, -1, 0, 0) # read-only, no window
            $document.SaveAs($outputPath, 32) # ppSaveAsPDF
        }
    }
} catch {
    $failure = $_.Exception.Message
} finally {
    # Open can throw after creating the document. Recover only our exact staged
    # path, never an arbitrary active document or a user's existing document.
    if ($null -eq $document -and $null -ne $documents) {
        try {
            for ($index = $documents.Count; $index -ge 1; $index--) {
                $candidate = $documents.Item($index)
                if ($candidate.FullName -eq $inputPath -or $candidate.FullName -eq $outputPath) {
                    $document = $candidate
                    break
                }
                [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($candidate)
            }
        } catch {
            $cleanupOk = $false
            [Console]::Error.WriteLine("Could not locate staged document for cleanup: $($_.Exception.Message)")
        }
    }
    if ($null -ne $document) {
        try {
            if ($appName -eq 'PowerPoint') {
                $document.Saved = -1
                $document.Close()
            } elseif ($appName -eq 'Word') {
                $document.Close(0) # wdDoNotSaveChanges
            } else {
                $document.Close($false)
            }
        } catch {
            $cleanupOk = $false
            [Console]::Error.WriteLine("Could not close staged document: $($_.Exception.Message)")
        }
    }
    if ($null -ne $application -and $null -ne $state) {
        # Restore independently so one failed setter does not skip the others.
        foreach ($property in @('AutomationSecurity', 'DisplayAlerts', 'EnableEvents')) {
            if ($state.Contains($property)) {
                try { $application.$property = $state[$property] }
                catch {
                    $cleanupOk = $false
                    [Console]::Error.WriteLine("Could not restore ${property}: $($_.Exception.Message)")
                }
            }
        }
    }
    if ($mayQuit -and $null -ne $documents -and $cleanupOk) {
        try {
            # Never close other documents, including anything opened meanwhile.
            if ($documents.Count -eq 0) { $application.Quit() }
        } catch {
            $cleanupOk = $false
            [Console]::Error.WriteLine("Could not close automation instance: $($_.Exception.Message)")
        }
    }
    foreach ($reference in @($document, $documents, $application)) {
        if ($null -ne $reference -and [Runtime.InteropServices.Marshal]::IsComObject($reference)) {
            try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($reference) }
            catch { $cleanupOk = $false }
        }
    }
    if ($cleanupOk -and (Test-Path -LiteralPath $recoveryPath)) {
        Remove-Item -LiteralPath $recoveryPath -Force
    }
    if ($locked) { $mutex.ReleaseMutex() }
    if ($null -ne $mutex) { $mutex.Dispose() }
}
if ($null -ne $failure) {
    [Console]::Error.WriteLine($failure)
    exit 1
}
if (-not $cleanupOk) { exit 1 }
exit 0
