-- Emit coordinates only. Cell text and formulas stay inside Excel/this script.
set fieldSeparator to ASCII character 9
set layoutRecords to {"V" & fieldSeparator & "1"}
set worksheetIndex to 0
repeat with currentSheet in (get worksheets of ownedDoc)
    set worksheetIndex to worksheetIndex + 1
    if (get visible of currentSheet) is sheet visible then
        set conversionStage to "inspecting Excel layout on worksheet " & worksheetIndex
        activate object currentSheet
        set end of layoutRecords to "S" & fieldSeparator & worksheetIndex
        set scanRange to used range of currentSheet
        set rowCount to count of rows of scanRange
        set columnCount to count of columns of scanRange
        if rowCount * columnCount > 200000 then
            -- Never infer from a truncated grid: use complete width fitting.
            set end of layoutRecords to "K"
        else
            set firstRow to first row index of scanRange
            set firstColumn to first column index of scanRange
            set cellValues to formula of scanRange
            if rowCount * columnCount is 1 then set cellValues to {{cellValues}}
            set mergeFlags to get merge cells of every cell of scanRange
            set knownMerges to {}
            repeat with r from 1 to rowCount
                set rowValues to item r of cellValues
                repeat with c from 1 to columnCount
                    set rowIndex to firstRow + r - 1
                    set columnIndex to firstColumn + c - 1
                    set cellValue to item c of rowValues
                    if cellValue is not missing value and cellValue is not "" then
                        set end of layoutRecords to "C" & fieldSeparator & rowIndex & fieldSeparator & columnIndex
                    end if
                    set cellIndex to (r - 1) * columnCount + c
                    if item cellIndex of mergeFlags is true then
                        set alreadyKnown to false
                        repeat with m in knownMerges
                            if rowIndex >= item 1 of m and rowIndex <= item 3 of m and columnIndex >= item 2 of m and columnIndex <= item 4 of m then
                                set alreadyKnown to true
                                exit repeat
                            end if
                        end repeat
                        if not alreadyKnown then
                            set mergedRange to merge area of cell cellIndex of scanRange
                            set mt to first row index of mergedRange
                            set ml to first column index of mergedRange
                            set mb to mt + (count of rows of mergedRange) - 1
                            set mr to ml + (count of columns of mergedRange) - 1
                            set end of knownMerges to {mt, ml, mb, mr}
                            set end of layoutRecords to "M" & fieldSeparator & mt & fieldSeparator & ml & fieldSeparator & mb & fieldSeparator & mr
                        end if
                    end if
                end repeat
            end repeat
            repeat with artwork in (get shapes of currentSheet)
                if (get visible of artwork) then
                    set startCell to top left cell of artwork
                    set endCell to bottom right cell of artwork
                    set end of layoutRecords to "G" & fieldSeparator & (first row index of startCell) & fieldSeparator & (first column index of startCell) & fieldSeparator & (first row index of endCell) & fieldSeparator & (first column index of endCell)
                end if
            end repeat
            repeat with nativeTable in (get list objects of currentSheet)
                set tableRange to range object of nativeTable
                set mt to first row index of tableRange
                set ml to first column index of tableRange
                set mb to mt + (count of rows of tableRange) - 1
                set mr to ml + (count of columns of tableRange) - 1
                set end of layoutRecords to "T" & fieldSeparator & mt & fieldSeparator & ml & fieldSeparator & mb & fieldSeparator & mr
            end repeat
        end if
    end if
end repeat
set end of layoutRecords to "E"
set oldDelimiters to AppleScript's text item delimiters
set AppleScript's text item delimiters to linefeed
set layoutText to layoutRecords as text
set AppleScript's text item delimiters to oldDelimiters
my writeState(outputPath, layoutText)
