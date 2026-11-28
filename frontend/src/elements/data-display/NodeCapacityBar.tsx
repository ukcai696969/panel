import Progress from '@/elements/feedback/Progress.tsx';
import Text from '@/elements/typography/Text.tsx';
import { NODE_RESOURCE_WARN_RATIO } from '@/lib/domain/node.ts';
import { bytesToString, mbToBytes } from '@/lib/format/size.ts';

interface NodeCapacityBarProps {
  label: string;
  /** Used amount, in MB (allocated). */
  usedMb: number;
  /** Total amount, in MB. `0` means unlimited. */
  limitMb: number;
  className?: string;
}

/** A labelled capacity bar for one node resource (RAM or disk). Red at or above 85% used. */
export default function NodeCapacityBar({ label, usedMb, limitMb, className }: NodeCapacityBarProps) {
  const ratio = limitMb === 0 ? 0 : usedMb / limitMb;
  const color = ratio >= NODE_RESOURCE_WARN_RATIO ? 'red' : ratio >= 0.7 ? 'yellow' : 'green';

  return (
    <div className={className}>
      <div className='flex justify-between gap-2'>
        <Text size='xs'>{label}</Text>
        <Text size='xs'>
          {bytesToString(mbToBytes(usedMb))} / {limitMb === 0 ? '∞' : bytesToString(mbToBytes(limitMb))}
        </Text>
      </div>
      <Progress value={ratio * 100} color={color} hourglass={false} withLabel={false} />
    </div>
  );
}
