-- Default fitting and automatic sections only change the staged copy.
set requestedPrintAreas to __EXCEL_PRINT_AREAS__
set automaticPrintAreas to __EXCEL_AUTO_AREAS__
repeat with requestedArea in requestedPrintAreas
    set targetName to item 1 of requestedArea
    set targetFound to false
    repeat with currentSheet in (get worksheets of ownedDoc)
        if (get name of currentSheet) is targetName and (get visible of currentSheet) is sheet visible then set targetFound to true
    end repeat
    if not targetFound then error "Requested print area worksheet is missing or hidden: " & targetName
end repeat
if __EXCEL_FIT_WIDTH__ then
    -- These changes apply only to the private staged workbook, which is never saved.
    set conversionStage to "preparing Excel print areas"
    set emptySheets to {}
    set printableSheets to 0
    repeat with currentSheet in (get worksheets of ownedDoc)
        if (get visible of currentSheet) is sheet visible then
            set conversionStage to "finding printable content on Excel sheet " & (get name of currentSheet)
            -- Match Excel's active sheet to the range before using Find.
            activate object currentSheet
            set scanRange to used range of currentSheet
            set lastRowCell to missing value
            try
                set lastRowCell to find scanRange what "*" look in formulas look at part search order by rows search direction search previous match case false
            on error msg number num
                -- Excel for Mac reports -50 instead of missing value for no matches.
                if num is not -50 then error msg number num
            end try
            set hasContent to (lastRowCell is not missing value)
            set topRow to 1048576
            set leftColumn to 16384
            set bottomRow to 0
            set rightColumn to 0
            if hasContent then
                set lastColumnCell to find scanRange what "*" look in formulas look at part search order by columns search direction search previous match case false
                set firstRowCell to find scanRange what "*" after (last cell of scanRange) look in formulas look at part search order by rows search direction search next match case false
                set firstColumnCell to find scanRange what "*" after (last cell of scanRange) look in formulas look at part search order by columns search direction search next match case false
                set topRow to first row index of firstRowCell
                set leftColumn to first column index of firstColumnCell
                set bottomRow to (first row index of lastRowCell) + (count of rows of merge area of lastRowCell) - 1
                set rightColumn to (first column index of lastColumnCell) + (count of columns of merge area of lastColumnCell) - 1
            end if
            -- Images and charts must not be mistaken for empty worksheet content.
            repeat with artwork in (get shapes of currentSheet)
                if (get visible of artwork) then
                    set hasContent to true
                    set artworkCell to top left cell of artwork
                    if (first row index of artworkCell) < topRow then set topRow to first row index of artworkCell
                    if (first column index of artworkCell) < leftColumn then set leftColumn to first column index of artworkCell
                    set artworkCell to bottom right cell of artwork
                    if (first row index of artworkCell) > bottomRow then set bottomRow to first row index of artworkCell
                    if (first column index of artworkCell) > rightColumn then set rightColumn to first column index of artworkCell
                end if
            end repeat
            if hasContent then
                set printableSheets to printableSheets + 1
                -- Excel prompts when a print area contains exactly one cell. Add
                -- one empty column on the private copy; fit-to-width still keeps
                -- it on one page, without changing global alert preferences.
                if topRow is bottomRow and leftColumn is rightColumn then
                    if rightColumn < (count of columns of currentSheet) then
                        set rightColumn to rightColumn + 1
                    else
                        set leftColumn to leftColumn - 1
                    end if
                end if
                set conversionStage to "resetting page breaks on Excel sheet " & (get name of currentSheet)
                reset all page breaks currentSheet
                set pageSettings to page setup object of currentSheet
                set conversionStage to "setting fit mode on Excel sheet " & (get name of currentSheet)
                set zoom of pageSettings to false
                set conversionStage to "setting fit width on Excel sheet " & (get name of currentSheet)
                set fit to pages wide of pageSettings to 1
                -- Excel's macOS setter rejects both 0 and false for automatic height.
                -- A large height limit lets width determine the scale for long sheets.
                set fit to pages tall of pageSettings to 32767
                set conversionStage to "locating print start on Excel sheet " & (get name of currentSheet)
                set firstAddress to get address (cell topRow of column leftColumn of currentSheet)
                set conversionStage to "locating print end on Excel sheet " & (get name of currentSheet)
                set lastAddress to get address (cell bottomRow of column rightColumn of currentSheet)
                set conversionStage to "setting Excel print area " & firstAddress & ":" & lastAddress
                set print area of pageSettings to firstAddress & ":" & lastAddress
            else
                set end of emptySheets to contents of currentSheet
            end if
        end if
    end repeat
    if printableSheets is 0 then error "Workbook has no printable visible worksheets"
    repeat with emptySheet in emptySheets
        set visible of emptySheet to sheet hidden
    end repeat
end if
-- Excel's whole-workbook PDF exporter can omit sheets and reuse the wrong
-- page size when the workbook mixes paper formats. Export each visible sheet
-- in the original workbook so formulas and cross-sheet references stay intact.
set visibleSheets to {}
repeat with currentSheet in (get worksheets of ownedDoc)
    if (get visible of currentSheet) is sheet visible then set end of visibleSheets to contents of currentSheet
end repeat
if (count of visibleSheets) is 0 then error "Workbook has no visible worksheets"
set sheetIndex to 0
repeat with currentSheet in visibleSheets
    set conversionStage to "exporting Excel sheet " & (get name of currentSheet)
    -- Make the next sheet visible before hiding the previous one. All changes
    -- are confined to the private workbook, which is closed without saving.
    set visible of currentSheet to sheet visible
    activate object currentSheet
    repeat with otherSheet in visibleSheets
        if (get name of otherSheet) is not (get name of currentSheet) then set visible of otherSheet to sheet hidden
    end repeat
    set sheetAreas to {}
    repeat with requestedArea in requestedPrintAreas
        if (item 1 of requestedArea) is (get name of currentSheet) then set end of sheetAreas to item 2 of requestedArea
    end repeat
    if (count of sheetAreas) is 0 then
        repeat with automaticArea in automaticPrintAreas
            if (get name of worksheet (item 1 of automaticArea) of ownedDoc) is (get name of currentSheet) then set end of sheetAreas to item 2 of automaticArea
        end repeat
    end if
    if (count of sheetAreas) is 0 then set sheetAreas to {missing value}
    repeat with currentArea in sheetAreas
        if (contents of currentArea) is not missing value then
            set conversionStage to "setting print section on Excel sheet " & (get name of currentSheet)
            set print area of (page setup object of currentSheet) to contents of currentArea
        end if
        set sheetIndex to sheetIndex + 1
        set conversionStage to "exporting Excel sheet " & (get name of currentSheet)
        save workbook as ownedDoc filename (outputPath & "." & sheetIndex & ".pdf") file format PDF file format
    end repeat
end repeat
my writeState(outputPath, sheetIndex as text)
