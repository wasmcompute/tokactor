#!/bin/bash

# The script takes 2 file arguments
# - 1: a JSON file
# - 2: a JSON file in the same format

FILE=$1
OLD_FILE=$2

if [[ -z "$OLD_FILE" ]]; then
    OLD_FILE=$1
fi
if [[ ! -f "$OLD_FILE" ]]; then
    OLD_FILE=$1
fi

OLD_TOTALS=$(cat $OLD_FILE | jq '.data[0].totals')


TOTALS=$(cat $FILE | jq '.data[0].totals')
KEYS=$(cat $FILE | jq -r '.data[0].totals' | jq -rj 'keys')

echo "| Coverage | Statistic | New Coverage | Old Coverage | Delta |"
echo "| --- | --- | --- | --- | --- |"

echo $KEYS | jq -cr '.[]' | while read key; do
    OBJ=$(echo $TOTALS | jq -r ".$key")
    COUNT=$(echo $OBJ | jq -r '.count')
    COVERED=$(echo $OBJ | jq -r '.covered')
    PERCENT=$(echo $OBJ | jq -r '.percent' | jq '.*100.0 | round / 100.0' )

    OLD_OBJ=$(echo $OLD_TOTALS | jq -r ".$key")
    OLD_COUNT=$(echo $OLD_OBJ | jq -r '.count')
    OLD_COVERED=$(echo $OLD_OBJ | jq -r '.covered')
    OLD_PERCENT=$(echo $OLD_OBJ | jq -r '.percent' | jq '.*100.0 | round / 100.0' )

    statistic="Count <br> Covered <br> Percent"
    coverage="\`$COUNT\` <br> \`$COVERED\` <br> \`$PERCENT\`"
    old_coverage="\`$OLD_COUNT\` <br> \`$OLD_COVERED\` <br> \`$OLD_PERCENT\`"

    DELTA_COUNT=$(echo $COUNT - $OLD_COUNT | bc)
    COUNT_TEXT=":white_check_mark: \`$DELTA_COUNT\`"
    if [[ $DELTA_COUNT -lt 0 ]]; then
        COUNT_TEXT=":x: \`$DELTA_COUNT\`"
    fi

    DELTA_COVERED=$(echo $COVERED - $OLD_COVERED | bc)
    COVERED_TEXT=":white_check_mark: \`$DELTA_COVERED\`"
    if [[ $DELTA_COVERED -lt 0 ]]; then
        COVERED_TEXT=":x: \`$DELTA_COVERED\`"
    fi

    DELTA_PERCENT=$(echo "$PERCENT * 100 - $OLD_PERCENT * 100" | bc | jq 'round / 100.0')
    PERCENT_TEXT=":white_check_mark: \`$DELTA_PERCENT\`"
    IS_LESS=$(echo "$DELTA_PERCENT < 0" | bc -l)
    if [[ $IS_LESS -eq 1 ]]; then
        PERCENT_TEXT=":x: \`$DELTA_PERCENT\`"
    fi

    DELTA="$COUNT_TEXT <br> $COVERED_TEXT <br> $PERCENT_TEXT"

    echo "| $key | $statistic | $coverage | $old_coverage | $DELTA |"
done
