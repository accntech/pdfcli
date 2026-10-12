on writeState(statePath, stateValue)
    set stateHandle to open for access (POSIX file statePath) with write permission
    try
        set eof stateHandle to 0
        write stateValue to stateHandle as «class utf8»
        close access stateHandle
    on error msg number num
        try
            close access stateHandle
        end try
        error msg number num
    end try
end writeState

on run argv
    set inputPath to item 1 of argv
    set outputPath to item 2 of argv
    set statePath to item 3 of argv
    set timeoutSeconds to (item 4 of argv) as integer
    set cleanupOnly to (item 5 of argv is "cleanup")
    set inputFile to POSIX file inputPath
    set inputName to item 6 of argv
    set inputHfs to inputFile as text
    set outputHfs to (POSIX file outputPath) as text
    set ownedDoc to missing value
    set restoreNeeded to false
    set previousLevel to 0
    set conversionStage to "starting automation"
    tell application id "__APP_ID__"
        try
            if cleanupOnly then
                try
                    set previousLevel to (read (POSIX file statePath) as «class utf8») as integer
                end try
                set restoreNeeded to (previousLevel > 0)
            else
                with timeout of timeoutSeconds seconds
                    set conversionStage to "reading macro settings"
                    set oldSecurity to automation security
                    if oldSecurity is msoAutomationSecurityLow then
                        set previousLevel to 1
                    else if oldSecurity is msoAutomationSecurityByUI then
                        set previousLevel to 2
                    else if oldSecurity is msoAutomationSecurityForceDisable then
                        set previousLevel to 3
                    else if oldSecurity is missing value and __ALLOW_MISSING_SECURITY__ then
                        -- Modern PowerPoint advertises this property but may not implement it.
                        -- Leave its existing macro policy untouched when it cannot be restored.
                        set previousLevel to 0
                    else
                        error "Unrecognized Office automation security setting"
                    end if
                    set conversionStage to "recording macro settings"
                    my writeState(statePath, previousLevel as text)
                    set restoreNeeded to (previousLevel > 0)
                    if restoreNeeded then
                        set conversionStage to "disabling macros"
                        set automation security to msoAutomationSecurityForceDisable
                    end if
                    set conversionStage to "opening document"
                    __OPEN__
                    repeat with candidate in (get __DOCUMENTS__)
                        set resolvedPath to ""
                        try
                            set ownedPath to full name of candidate
                            set resolvedPath to ownedPath
                            if ownedPath does not start with "/" then set resolvedPath to POSIX path of (ownedPath as alias)
                        end try
                        if resolvedPath is inputPath then
                            set ownedDoc to contents of candidate
                            exit repeat
                        end if
                    end repeat
                    if ownedDoc is missing value then error "Opened Office document was not found by its staged path"
                    set conversionStage to "restoring macro settings"
                    if restoreNeeded then set automation security to oldSecurity
                    set restoreNeeded to false
                    my writeState(statePath, "0")
                    set conversionStage to "exporting PDF"
                    __SAVE__
                    set conversionStage to "closing document"
                    close ownedDoc saving no
                    set ownedDoc to missing value
                end timeout
            end if
        on error msg number num
            -- Restoration and document cleanup also run from Rust after failure.
            error (conversionStage & ": " & msg) number num
        end try
        if cleanupOnly then
            with timeout of 5 seconds
                if restoreNeeded then
                    if previousLevel is 1 then
                        set automation security to msoAutomationSecurityLow
                    else if previousLevel is 2 then
                        set automation security to msoAutomationSecurityByUI
                    else if previousLevel is 3 then
                        set automation security to msoAutomationSecurityForceDisable
                    end if
                    my writeState(statePath, "0")
                end if
                repeat with candidate in (get __DOCUMENTS__)
                    set candidatePath to ""
                    try
                        set candidatePath to full name of candidate
                        if candidatePath does not start with "/" then set candidatePath to POSIX path of (candidatePath as alias)
                    end try
                    if candidatePath is inputHfs or candidatePath is inputPath or candidatePath is outputHfs or candidatePath is outputPath then
                        close (contents of candidate) saving no
                    end if
                end repeat
            end timeout
        end if
    end tell
end run
