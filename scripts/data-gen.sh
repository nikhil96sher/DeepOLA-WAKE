if [ "$#" -ne 3 ]; then
	echo "Usage: ./data-gen.sh <scale> <partition> <base-dir>. Example: ./data-gen.sh 1 10 $PWD/.."
	exit
fi
scale=$1
partition=$2
BASE_DIR=$3
export DSS_PATH=$BASE_DIR/resources/tpc-h/data/scale=$scale/partition=$partition/tbl
mkdir -p $DSS_PATH
echo $DSS_PATH
cd ../tpch-kit/dbgen/
if [ $partition -eq 1 ];
then
	echo "Running"
	./dbgen -f -s $scale -v
else
	for chunk in $(seq 1 $partition)
		do
			echo $chunk
			./dbgen -f -s $scale -S $chunk -C $partition -v
		done
fi
