on run argv
    set dstAlias to POSIX file (item 1 of argv) as alias
    set srcAlias to POSIX file (item 2 of argv) as alias
    set dstName to name of (info for dstAlias)
    set srcName to name of (info for srcAlias)
    set includeBookmarks to (item 3 of argv is "true")
    tell application "/Applications/Adobe Acrobat DC/Adobe Acrobat.app"
        with timeout of 120 seconds
            try
                open dstAlias invisible true
                open srcAlias invisible true
                insert pages document dstName after 1 from document srcName starting with 1 number of pages 2 insert bookmarks includeBookmarks
                save document dstName
                set resultCount to count pages of document dstName
                close document srcName saving no
                close document dstName saving no
                return resultCount
            on error errorMessage number errorNumber
                try
                    close document srcName saving no
                end try
                try
                    close document dstName saving no
                end try
                error errorMessage number errorNumber
            end try
        end timeout
    end tell
end run
