import { Button, Field, Input } from '@fluentui/react-components';
import { DismissRegular, SearchRegular } from '@fluentui/react-icons';
import { useRuleListPageStyles } from '@/styles/ruleListPage';

type RuleListSearchBarProps = {
  value: string;
  placeholder: string;
  onChange: (value: string) => void;
};

export function RuleListSearchBar({ value, placeholder, onChange }: RuleListSearchBarProps) {
  const listPage = useRuleListPageStyles();

  return (
    <div className={listPage.toolbar}>
      <Field label="Search" className={listPage.searchField}>
        <Input
          value={value}
          placeholder={placeholder}
          contentBefore={<SearchRegular />}
          onChange={(_, data) => onChange(data.value)}
        />
      </Field>
      <Button
        appearance="subtle"
        icon={<DismissRegular />}
        disabled={value.trim() === ''}
        onClick={() => onChange('')}
      >
        Clear
      </Button>
    </div>
  );
}
